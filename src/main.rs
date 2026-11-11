use clap::Parser;
use std::{future::IntoFuture, net::SocketAddr, path::PathBuf, time::Duration};

#[derive(Parser)]
#[command(version, about = "Personal Markdown notes inside your tailnet")]
struct Options {
    #[arg(long, default_value = "./notes")]
    data_dir: PathBuf,
    #[arg(long, default_value = "127.0.0.1:8080")]
    listen: SocketAddr,
    #[arg(long)]
    origin: Option<String>,
    #[arg(long, conflicts_with_all = ["backup_to", "restore_from"])]
    export_to: Option<PathBuf>,
    #[arg(long, conflicts_with_all = ["export_to", "restore_from"])]
    backup_to: Option<PathBuf>,
    #[arg(long, conflicts_with_all = ["export_to", "backup_to"])]
    restore_from: Option<PathBuf>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(4)
        .enable_all()
        .build()?;
    let result = runtime.block_on(run());
    runtime.shutdown_timeout(Duration::from_secs(3));
    result
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let options = Options::parse();
    if let Some(input) = options.restore_from {
        tungsten::store::Store::restore_from(&input, &options.data_dir)?;
        eprintln!("operation=restore outcome=complete");
        return Ok(());
    }
    if let Some(output) = options.backup_to {
        tungsten::store::Store::open(&options.data_dir)?.backup_to(&output)?;
        eprintln!("operation=backup outcome=complete");
        return Ok(());
    }
    if let Some(output) = options.export_to {
        tungsten::export::write(&options.data_dir, &output)?;
        eprintln!("operation=export outcome=complete");
        return Ok(());
    }
    let app = match options.origin {
        Some(origin) => tungsten::api::app_with_origin(
            &options.data_dir,
            tungsten::api::ServerOrigin::https(&origin)?,
        )?,
        None => tungsten::api::app(&options.data_dir)?,
    };
    let listener = tokio::net::TcpListener::bind(options.listen).await?;
    eprintln!(
        "operation=listen outcome=ready address={}",
        listener.local_addr()?
    );
    let (stop, receiver) = tokio::sync::oneshot::channel();
    let serving = axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = receiver.await;
        })
        .into_future();
    tokio::pin!(serving);
    tokio::select! {
        result = &mut serving => result?,
        () = shutdown() => {
            let _ = stop.send(());
            match tokio::time::timeout(Duration::from_secs(2), &mut serving).await {
                Ok(result) => result?,
                Err(_) => eprintln!("operation=shutdown outcome=request_deadline"),
            }
        }
    }
    Ok(())
}

async fn shutdown() {
    let interrupt = tokio::signal::ctrl_c();
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => {
                eprintln!("operation=signal outcome=failed error={error}");
            }
        }
    };
    tokio::select! {
        result = interrupt => { if let Err(error) = result { eprintln!("operation=signal outcome=failed error={error}"); } },
        () = terminate => {},
    }
}
