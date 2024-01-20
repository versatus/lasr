use lasr::{OciBundler, OciManager, ensure_dir_exists};
use futures::stream::{FuturesUnordered, StreamExt};

#[tokio::main]
async fn main() -> Result<(), std::io::Error> {
    simple_logger::init_with_level(
        log::Level::Info
    ).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    let mut results = FuturesUnordered::new();
    let containers_path = "./containers";
    let base_image_path = "./base_image";
    let payload_path = "./payload";
    ensure_dir_exists(containers_path)?;
    ensure_dir_exists(base_image_path)?;
    ensure_dir_exists(payload_path)?;
    // setup container bundler, this should be an actor asynchronously 
    // running in it own thread having messages passed back and forth
    let bundler: OciBundler<String, String> = OciBundler::new(
        //TODO: move to .env file
        "/usr/local/bin/runsc".to_string(),
        containers_path.to_string(),
        base_image_path.to_string(),
        payload_path.to_string()
    );

    let manager = OciManager::new(
        bundler
    );

    let task_manager_1 = manager.clone();
    let py_handle = tokio::task::spawn(async move {
        task_manager_1.bundle("testContainerPy", lasr::BaseImage::Bin).await?;
        task_manager_1.add_payload("testContainerPy").await?;
        task_manager_1.base_spec("testContainerPy").await?;
        task_manager_1.customize_spec("testContainerPy", "/hello-world", None)?;

        let start = std::time::Instant::now();
        let _ = task_manager_1.run_container(
            "testContainerPy",
            "getName".to_string(),
            vec!["Andrew".to_string(), "Smith".to_string()]
        ).await?.await??;
        let elapsed = start.elapsed();
        log::info!("testContainerPy ran in: {:?}", elapsed);
        Ok::<_, std::io::Error>(())
    });

    results.push(py_handle);

    let task_manager_2 = manager.clone();
    
    let rs_handle = tokio::task::spawn(async move {
        task_manager_2.bundle("testContainerRs", lasr::BaseImage::Bin).await?;
        task_manager_2.add_payload("testContainerRs").await?;
        task_manager_2.base_spec("testContainerRs").await?;
        task_manager_2.customize_spec("testContainerRs", "/hello-world", None)?;

        let start = std::time::Instant::now();
        let _ = task_manager_2.run_container("testContainerRs", "".to_string(), vec![]).await?.await??;
        let elapsed = start.elapsed();
        log::info!("testContainerRs ran in: {:?}", elapsed);

        Ok::<_, std::io::Error>(())
    });

    results.push(rs_handle);

    while results.len() > 0 {
        tokio::select! {
            res = results.next() => {
                match res {
                    Some(Ok(r)) => {
                        log::info!("future completed successfully: {:?}", r);
                    }
                    _ => {}
                }
            }
        }
    }
    
    Ok(())
}
