use actix_web::{
    http::Method, server::HttpServer, App, AsyncResponder, Error, HttpMessage, HttpRequest,
    HttpResponse, Result,
};
use serde_derive::{Deserialize, Serialize};
use time::{now_utc, strftime};

use actix::prelude::*;
use futures::future::Future;
use redis::{Client, Commands, Connection};

struct State {
    db: Addr<DbExecutor>,
}

struct DbExecutor(Connection);

impl Actor for DbExecutor {
    type Context = SyncContext<Self>;
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct NewRecord {
    phone: String,
    first_name: String,
    last_name: String,
}

struct GetRecord {
    phone: String,
}

impl Message for NewRecord {
    type Result = Result<(), Error>;
}

impl Message for GetRecord {
    type Result = Result<Option<Record>, Error>;
}

impl Handler<NewRecord> for DbExecutor {
    type Result = Result<(), Error>;

    fn handle(&mut self, msg: NewRecord, _: &mut Self::Context) -> Self::Result {
        let (phone, record) = {
            let (phone, fist_name, last_name) = (msg.phone, msg.first_name, msg.last_name);
            let created_at = now_iso_8601();
            (
                phone,
                serde_json::to_string(&Record {
                    fist_name,
                    last_name,
                    created_at,
                })
                .expect("can not encode record"),
            )
        };
        let _: () = self.0.set(phone, record).expect("can not write to redis");
        Ok(())
    }
}

impl Handler<GetRecord> for DbExecutor {
    type Result = Result<Option<Record>, Error>;

    fn handle(&mut self, msg: GetRecord, _: &mut Self::Context) -> Self::Result {
        Ok(self
            .0
            .get(msg.phone)
            .map(|value: String| serde_json::from_str(&value).ok())
            .unwrap_or(None))
    }
}

fn now_iso_8601() -> String {
    strftime("%Y-%m-%dT%H:%M:%SZ", &now_utc()).expect("invalid created_at")
}

#[derive(Serialize, Deserialize)]
struct Record {
    fist_name: String,
    last_name: String,
    created_at: String,
}

fn index(_req: &HttpRequest<State>) -> &'static str {
    "actix-redis-example"
}

fn info(req: &HttpRequest<State>) -> Box<Future<Item = HttpResponse, Error = Error>> {
    let phone = req.match_info()["phone"].to_string();
    req.state()
        .db
        .send(GetRecord { phone })
        .from_err()
        .and_then(|res| match res {
            Ok(maybe_record) => match maybe_record {
                Some(record) => Ok(HttpResponse::Ok().json(record)),
                None => Ok(HttpResponse::NotFound().into()),
            },
            Err(_) => Ok(HttpResponse::InternalServerError().into()),
        })
        .responder()
}

fn add(req: &HttpRequest<State>) -> Box<Future<Item = HttpResponse, Error = Error>> {
    let db = req.state().db.clone();
    req.json()
        .from_err()
        .and_then(move |record: NewRecord| {
            let record2 = record.clone();
            db.send(record).from_err().and_then(|res| match res {
                Ok(_) => Ok(HttpResponse::Ok().json(record2)),
                Err(_) => Ok(HttpResponse::InternalServerError().into()),
            })
        })
        .responder()
}

fn main() {
    let sys = actix::System::new("actix-redis-example");

    let addr = SyncArbiter::start(5, || {
        let client = Client::open("redis://127.0.0.1/").expect("can not create redis client");
        let con = client.get_connection().expect("can not connect to redis");
        DbExecutor(con)
    });

    HttpServer::new(move || {
        App::with_state(State { db: addr.clone() })
            .resource("/phone/{phone}", |r| r.method(Method::GET).a(info))
            .resource("/phone", |r| r.method(Method::POST).a(add))
            .resource("/", |r| r.f(index))
            .finish()
    })
    .bind("127.0.0.1:8088")
    .expect("can not bind 127.0.0.1:8088")
    .keep_alive(60)
    .start();

    println!("start http server: 127.0.0.1:8088");
    let _ = sys.run();
}
