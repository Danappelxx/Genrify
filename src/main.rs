use actix_web::{get, web, App, HttpServer, Responder};

#[get("/")]
async fn index() -> impl Responder {
    "Hello, world!!!"
}

#[get("/{name}")]
async fn name(info: web::Path<String>) -> impl Responder {
    format!("Hello {}!", info)
}

#[actix_rt::main]
async fn main() -> std::io::Result<()> {
    HttpServer::new(|| {
        App::new()
            .service(index)
            .service(name)
    })
    .bind("0.0.0.0:8080")?
    .run()
    .await
}
