use gevulot_rs::builders::{ByteSize, ByteUnit, MsgCreateWorkerBuilder, MsgDeleteWorkerBuilder};

use crate::{connect_to_gevulot, print_object, read_file};

/// Lists all workers.
///
/// # Arguments
///
/// * `_sub_m` - A reference to the ArgMatches struct containing parsed command-line arguments.
///              This is not used directly in the function but is passed to other utility functions.
///
/// # Returns
///
/// A Result containing () if successful, or a Box<dyn std::error::Error> if an error occurs.
pub async fn list_workers(_sub_m: &clap::ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_to_gevulot(_sub_m).await?;
    let workers = client.workers.list().await?;
    let workers: Vec<gevulot_rs::models::Worker> = workers.into_iter().map(Into::into).collect();
    print_object(_sub_m, &workers)?;
    Ok(())
}

/// Retrieves a specific worker by ID.
///
/// # Arguments
///
/// * `_sub_m` - A reference to the ArgMatches struct containing parsed command-line arguments.
///              This includes the "id" argument specifying which worker to retrieve.
///
/// # Returns
///
/// A Result containing () if successful, or a Box<dyn std::error::Error> if an error occurs.
pub async fn get_worker(_sub_m: &clap::ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_to_gevulot(_sub_m).await?;
    if let Some(worker_id) = _sub_m.get_one::<String>("id") {
        let worker = client.workers.get(worker_id).await?;
        let worker: gevulot_rs::models::Worker = worker.into();
        print_object(_sub_m, &worker)?;
    } else {
        println!("Worker ID is required");
    }
    Ok(())
}

/// Creates a new worker based on the provided configuration.
///
/// # Arguments
///
/// * `_sub_m` - A reference to the ArgMatches struct containing parsed command-line arguments.
///              This includes the file path for the worker configuration.
///
/// # Returns
///
/// A Result containing () if successful, or a Box<dyn std::error::Error> if an error occurs.
pub async fn create_worker(_sub_m: &clap::ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    let worker: gevulot_rs::models::Worker = read_file(_sub_m).await?;
    let mut client = connect_to_gevulot(_sub_m).await?;
    let me = client
        .base_client
        .write()
        .await
        .address
        .clone()
        .ok_or("No address found, did you set a mnemonic?")?;
    let resp = client
        .workers
        .create(
            MsgCreateWorkerBuilder::default()
                .creator(me)
                .name(worker.metadata.name)
                .description(worker.metadata.description)
                .tags(worker.metadata.tags.into_iter().map(Into::into).collect())
                .labels(worker.metadata.labels.into_iter().map(Into::into).collect())
                .cpus(worker.spec.cpus as u64)
                .gpus(worker.spec.gpus as u64)
                .memory(ByteSize::new(worker.spec.memory as u64, ByteUnit::Byte))
                .disk(ByteSize::new(worker.spec.disk as u64, ByteUnit::Byte))
                .into_message()?,
        )
        .await?;

    println!("{}", resp.id);
    Ok(())
}

/// Deletes a worker with the specified ID.
///
/// # Arguments
///
/// * `_sub_m` - A reference to the ArgMatches struct containing parsed command-line arguments.
///              This includes the "id" argument specifying which worker to delete.
///
/// # Returns
///
/// A Result containing () if successful, or a Box<dyn std::error::Error> if an error occurs.
pub async fn delete_worker(_sub_m: &clap::ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    println!("Deleting a worker");
    let worker_id = _sub_m
        .get_one::<String>("id")
        .ok_or("Worker ID is required")?;
    let mut client = connect_to_gevulot(_sub_m).await?;
    let me = client
        .base_client
        .write()
        .await
        .address
        .clone()
        .ok_or("No address found, did you set a mnemonic?")?;

    client
        .workers
        .delete(
            MsgDeleteWorkerBuilder::default()
                .creator(me.clone())
                .id(worker_id.clone())
                .into_message()?,
        )
        .await?;

    println!("deleted {}", worker_id);
    Ok(())
}
