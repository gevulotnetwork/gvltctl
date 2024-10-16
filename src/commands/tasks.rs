use std::collections::HashMap;

use gevulot_rs::builders::{ByteSize, ByteUnit, MsgCreateTaskBuilder};

use crate::{connect_to_gevulot, print_object, read_file};

/// Lists all tasks.
///
/// # Arguments
///
/// * `_sub_m` - A reference to the ArgMatches struct containing parsed command-line arguments.
///              This is used to connect to Gevulot and determine the output format.
///
/// # Returns
///
/// A Result indicating success or an error if the task listing fails.
pub async fn list_tasks(_sub_m: &clap::ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_to_gevulot(_sub_m).await?;
    let tasks = client.tasks.list().await?;
    let tasks: Vec<gevulot_rs::models::Task> = tasks.into_iter().map(Into::into).collect();
    print_object(_sub_m, &tasks)?;
    Ok(())
}

/// Retrieves and displays information for a specific task.
///
/// # Arguments
///
/// * `_sub_m` - A reference to the ArgMatches struct containing parsed command-line arguments.
///              This includes the task ID to retrieve and is used to connect to Gevulot and determine the output format.
///
/// # Returns
///
/// A Result indicating success or an error if the task retrieval fails.
pub async fn get_task(_sub_m: &clap::ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = crate::connect_to_gevulot(_sub_m).await?;
    if let Some(task_id) = _sub_m.get_one::<String>("id") {
        let task = client.tasks.get(task_id).await?;
        let task: gevulot_rs::models::Task = task.into();
        print_object(_sub_m, &task)?;
    } else {
        println!("Task ID is required");
    }
    Ok(())
}

/// Creates a new task based on the provided specification.
///
/// # Arguments
///
/// * `_sub_m` - A reference to the ArgMatches struct containing parsed command-line arguments.
///              This is used to read the task specification file, connect to Gevulot, and determine the output format.
///
/// # Returns
///
/// A Result indicating success or an error if the task creation fails.
pub async fn create_task(_sub_m: &clap::ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    let task: gevulot_rs::models::Task = read_file(_sub_m).await?;
    let mut client = connect_to_gevulot(_sub_m).await?;
    let me = client
        .base_client
        .write()
        .await
        .address
        .clone()
        .ok_or("No address found, did you set a mnemonic?")?;

    let env: HashMap<String, String> = task
        .spec
        .env
        .iter()
        .map(|e| (e.name.clone(), e.value.clone()))
        .collect();

    let input_contexts: HashMap<String, String> = task
        .spec
        .input_contexts
        .iter()
        .map(|e| (e.source.clone(), e.target.clone()))
        .collect();

    let resp = client
        .tasks
        .create(
            MsgCreateTaskBuilder::default()
                .creator(me.clone())
                .image(task.spec.image)
                .command(task.spec.command)
                .args(task.spec.args)
                .env(env)
                .input_contexts(input_contexts)
                .output_contexts(
                    task.spec
                        .output_contexts
                        .into_iter()
                        .map(|oc| (oc.source, oc.retention_period as u64))
                        .collect(),
                )
                .cpus(task.spec.resources.cpus as u64)
                .gpus(task.spec.resources.gpus as u64)
                .memory(ByteSize::new(
                    task.spec.resources.memory as u64,
                    ByteUnit::Byte,
                ))
                .time(task.spec.resources.time as u64)
                .store_stdout(task.spec.store_stdout.unwrap_or(false))
                .store_stderr(task.spec.store_stderr.unwrap_or(false))
                .into_message()?,
        )
        .await?;

    println!("Created task with ID: {}", resp.id);
    Ok(())
}
