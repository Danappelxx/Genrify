use actix_web::{get, middleware::Logger, web, App, HttpServer, HttpResponse, Error, http, Responder, Either};
use actix_session::{CookieSession, Session};
use actix_files::Files;
use serde::{Serialize, Deserialize};
use std::env;
use std::collections::HashMap;
use rspotify::client::Spotify;
use rspotify::oauth2::{SpotifyOAuth, SpotifyClientCredentials, TokenInfo};
use rspotify::model::{track::*, audio::*};
use rspotify::util;

struct AppState {
    spotify_oauth: SpotifyOAuth,
}

#[get("/auth")]
async fn spotify_redir(data: web::Data<AppState>) -> impl Responder {
    let state = util::generate_random_string(16);
    let auth_url = data.spotify_oauth.get_authorize_url(Some(&state), None);
    HttpResponse::Found()
        .header(http::header::LOCATION, auth_url)
        .finish()
        .into_body()
}

#[derive(Debug, Deserialize)]
struct SpotifyCallback {
    state: String,
    code: Option<String>,
    error: Option<String>,
}

#[get("/spotify")]
async fn spotify_callback(
    session: Session,
    data: web::Data<AppState>,
    query: web::Query<SpotifyCallback>) -> Either<String, HttpResponse> {

    if let Some(error) = &query.error {
        println!("spotify auth error: {:?}", error);
        return Either::A("Failed to authorize.".to_owned());
    }

    let code = query.code.as_ref().unwrap();

    let token_info = match data.spotify_oauth.get_access_token(code).await {
        Some(token_info) => token_info,
        None => return Either::A("Bad authorization code.".to_owned()),
    };

    println!("token info: {:?}", token_info);

    if let Err(error) = session.set("token_info", token_info) {
        println!("error setting cookie: {:?}", error);
        return Either::A("Internal error.".to_owned());
    }

    Either::B(HttpResponse::Found()
        .header(http::header::LOCATION, "/")
        .finish()
        .into_body())
}

#[derive(Debug, Serialize)]
struct TrackAnalysis {
    track: SavedTrack,
    genres: Vec<String>,
    audio_features: AudioFeatures,
}

#[derive(Debug, Serialize)]
struct UserAnalysis {
    tracks: Vec<TrackAnalysis>,
    limit: u32,
    offset: u32,
    total: u32,
}

async fn fetch_user_analysis(spotify: Spotify, limit: u32, offset: u32) -> Result<UserAnalysis, Error> {
    let page = spotify.current_user_saved_tracks(limit, offset).await?;
    let tracks = page.items;
    let artist_ids: Vec<String> = tracks
        .iter()
        // flatten Vec<Vec<Artist>> to Vec<Artist>
        .flat_map(|track| &track.track.artists)
        // drop id's that are Option::none
        .flat_map(|artist| artist.id.as_ref().map(|id| id.clone()))
        .collect();
    let artists = spotify.artists(artist_ids).await?.artists;
    let artist_genres: HashMap<&String, &Vec<String>> = artists // [artist_uri:genres]
        .iter()
        .map(|artist| (&artist.uri, &artist.genres))
        .collect();
    let mut track_genres: HashMap<&String, Vec<&String>> = tracks // [track_uri:genres]
        .iter()
        .map(|track| {
            let genres: Vec<&String> = track.track.artists
                .iter()
                .flat_map(|artist| {
                    let artist_uri: &String = artist.uri.as_ref().unwrap();
                    let genres: &Vec<String> = artist_genres.get(artist_uri).unwrap();
                    return genres;
                })
                .collect();
            (&track.track.uri, genres)
        })
        .collect();
    let track_id_map: HashMap<String, &SavedTrack> = tracks
        .iter()
        .flat_map(|track| {
            let id = match &track.track.id {
                Some(id) => id,
                None => return None,
            };
            Some((id.clone(), track))
        })
        .collect();
    let track_ids: Vec<String> = track_id_map.keys().cloned().collect();
    let mut audios_features: HashMap<String, AudioFeatures> = spotify.audios_features(&track_ids[..]).await?
        .map(|features| features.audio_features)
        .unwrap_or(Vec::new())
        .into_iter()
        .flat_map(|features| {
            Some((features.id.clone(), features))
        })
        .collect();
    let tracks_analysis = tracks
        .iter()
        .flat_map(|track| {
            let track_id = match &track.track.id {
                Some(id) => id,
                None => return None,
            };
            // we remove from track_genres so we don't have to clone
            let genres = track_genres.remove(&track.track.uri)
                .unwrap_or(Vec::new())
                .into_iter()
                .cloned()
                .collect();
            // // we remove from audios_features so we don't have to clone
            let audio_features = match audios_features.remove(track_id) {
                Some(features) => features,
                None => return None,
            };
            Some(TrackAnalysis {
                track: track.clone(),
                genres,
                audio_features,
            })
        })
        .collect();
    Ok(UserAnalysis {
        tracks: tracks_analysis,
        limit,
        offset,
        total: page.total,
    })
}

#[get("/analysis")]
async fn analysis(session: Session) -> Result<Either<impl Responder, impl Responder>, Error> {
    let token_info: TokenInfo = match session.get::<TokenInfo>("token_info")? {
        Some(token_info) => token_info,
        None => return Ok(Either::A(HttpResponse::Unauthorized()
            .body("Not logged in.".to_owned()))),
    };
    println!("{:?}", token_info);
    let client_credential = SpotifyClientCredentials::default()
        .token_info(token_info)
        .build();
    let spotify = Spotify::default()
        .client_credentials_manager(client_credential)
        .build();
    let user_analysis = fetch_user_analysis(spotify, 10, 0).await?;
    Ok(Either::B(HttpResponse::Ok().json(user_analysis)))
}

#[actix_rt::main]
async fn main() -> std::io::Result<()> {
    env::set_var("RUST_LOG", "actix_web=debug,actix_server=info");
    env_logger::init();

    HttpServer::new(|| {
        let app_state = AppState {
            spotify_oauth: SpotifyOAuth::default()
                .scope("user-library-read playlist-modify-private")
                .build(),
        };

        App::new()
            .wrap(Logger::default())
            .wrap(CookieSession::signed(&[0; 32]).secure(false))
            .data(app_state)
            .service(spotify_redir)
            .service(spotify_callback)
            .service(analysis)
            .service(Files::new("/", "./public").index_file("index.html"))
    })
    .bind("0.0.0.0:8080")?
    .run()
    .await
}
