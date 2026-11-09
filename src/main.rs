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
    #[arg(long)]
    development: bool,
    #[arg(long, conflicts_with = "plaintext")]
    migrate_from: Option<PathBuf>,
    #[arg(long)]
    plaintext: bool,
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
    if options.development && !options.listen.ip().is_loopback() {
        return Err("Development mode requires a loopback listener.".into());
    }
    let listener = tokio::net::TcpListener::bind(options.listen).await?;
    let app = if !options.plaintext {
        let origin = if options.development {
            tungsten::api::ServerOrigin::development(&format!("http://{}", listener.local_addr()?))?
        } else {
            tungsten::api::ServerOrigin::https(options.origin.as_deref().ok_or(
                "Set --origin to a dedicated HTTPS origin, or use --development on loopback.",
            )?)?
        };
        if let Some(source) = options.migrate_from {
            tungsten::api::migration_app(&options.data_dir, &source, origin)?
        } else {
            tungsten::api::sealed_app(&options.data_dir, origin)?
        }
    } else {
        tungsten::api::app(&options.data_dir)?
    };
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
