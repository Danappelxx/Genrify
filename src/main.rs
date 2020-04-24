use actix_web::{get, middleware::Logger, web, App, HttpServer, HttpResponse, Error, http, Responder, Either};
use actix_session::{CookieSession, Session};
use actix_files::Files;
use serde::Deserialize;
use std::env;
use rspotify::client::Spotify;
use rspotify::oauth2::{SpotifyOAuth, SpotifyClientCredentials, TokenInfo};
use rspotify::util;

struct AppState {
    spotify_oauth: SpotifyOAuth,
}

#[derive(Debug, Deserialize)]
struct SpotifyCallback {
    state: String,
    code: Option<String>,
    error: Option<String>,
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

#[get("/analysis")]
async fn analysis(data: web::Data<AppState>, session: Session) -> Result<impl Responder, Error> {
    let token_info: TokenInfo = match session.get::<TokenInfo>("token_info")? {
        Some(token_info) => token_info,
        None => return Ok("Not logged in".to_owned()),
    };
    println!("{:?}", token_info);
    let client_credential = SpotifyClientCredentials::default()
        .token_info(token_info)
        .build();
    let spotify = Spotify::default()
        .client_credentials_manager(client_credential)
        .build();
    let tracks = spotify.current_user_saved_tracks(10, 0).await;
    println!("{:?}", tracks)
    Ok(format!("{:?}", tracks))
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
            .service(analysis)
            .service(spotify_callback)
            .service(Files::new("/", "./public").index_file("index.html"))
    })
    .bind("0.0.0.0:8080")?
    .run()
    .await
}
