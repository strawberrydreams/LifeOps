use lifeops_server::{serve, RunConfig};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().init();
    let (addr, fut) = serve(RunConfig::dev()).await.expect("서버 기동 실패");
    tracing::info!("LifeOps 서버 http://{addr}");
    fut.await;
}
