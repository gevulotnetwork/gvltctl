use gevulot_rs::proto::gevulot::gevulot::{
    MsgSudoDeletePin, MsgSudoDeleteWorker, MsgSudoDeleteTask, MsgSudoFreezeAccount,
};

use crate::connect_to_gevulot;

const OK: &str = "ok";

use clap::{Arg, Command, ValueHint};

pub fn get_command() -> clap::Command {
    Command::new("sudo")
        .about("Perform administrative operations with sudo privileges")
        .subcommand_required(true)
        .subcommand(
            Command::new("delete-pin")
                .about("Delete a pin using sudo privileges")
                .arg(
                    Arg::new("id")
                        .value_name("ID")
                        .help("The ID of the pin to delete")
                        .required(true)
                        .index(1)
                        .value_hint(ValueHint::Other),
                ),
        )
        .subcommand(
            Command::new("delete-worker")
                .about("Delete a worker using sudo privileges")
                .arg(
                    Arg::new("id")
                        .value_name("ID")
                        .help("The ID of the worker to delete")
                        .required(true)
                        .index(1)
                        .value_hint(ValueHint::Other),
                ),
        )
        .subcommand(
            Command::new("delete-task")
                .about("Delete a task using sudo privileges")
                .arg(
                    Arg::new("id")
                        .value_name("ID")
                        .help("The ID of the task to delete")
                        .required(true)
                        .index(1)
                        .value_hint(ValueHint::Other),
                ),
        )
        .subcommand(
            Command::new("freeze-account")
                .about("Freeze an account using sudo privileges")
                .arg(
                    Arg::new("address")
                        .value_name("ADDRESS")
                        .help("The address of the account to freeze")
                        .required(true)
                        .index(1)
                        .value_hint(ValueHint::Other),
                ),
        )
}


/// Deletes a pin using sudo privileges.
///
/// # Arguments
///
/// * `_sub_m` - A reference to the ArgMatches struct containing parsed command-line arguments.
///
/// # Returns
///
/// A Result containing () if successful, or a Box<dyn std::error::Error> if an error occurs.
pub async fn sudo_delete_pin(_sub_m: &clap::ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_to_gevulot(_sub_m).await?;
    if let Some(pin_id) = _sub_m.get_one::<String>("id") {
        let msg = MsgSudoDeletePin {
            authority: client.base_client.write().await.address.clone().unwrap(),
            cid: pin_id.clone(),
        };
        client.sudo.delete_pin(msg).await?;
        println!("{}", OK);
    } else {
        println!("Pin ID is required");
    }
    Ok(())
}

/// Deletes a worker using sudo privileges.
///
/// # Arguments
///
/// * `_sub_m` - A reference to the ArgMatches struct containing parsed command-line arguments.
///
/// # Returns
///
/// A Result containing () if successful, or a Box<dyn std::error::Error> if an error occurs.
pub async fn sudo_delete_worker(_sub_m: &clap::ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_to_gevulot(_sub_m).await?;
    if let Some(worker_id) = _sub_m.get_one::<String>("id") {
        let msg = MsgSudoDeleteWorker {
            authority: client.base_client.write().await.address.clone().unwrap(),
            id: worker_id.clone(),
        };
        client.sudo.delete_worker(msg).await?;
        println!("{}", OK);
    } else {
        println!("Worker ID is required");
    }
    Ok(())
}

/// Deletes a task using sudo privileges.
///
/// # Arguments
///
/// * `_sub_m` - A reference to the ArgMatches struct containing parsed command-line arguments.
///
/// # Returns
///
/// A Result containing () if successful, or a Box<dyn std::error::Error> if an error occurs.
pub async fn sudo_delete_task(_sub_m: &clap::ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_to_gevulot(_sub_m).await?;
    if let Some(task_id) = _sub_m.get_one::<String>("id") {
        let msg = MsgSudoDeleteTask {
            authority: client.base_client.write().await.address.clone().unwrap(),
            id: task_id.clone(),
        };
        client.sudo.delete_task(msg).await?;
        println!("{}", OK);
    } else {
        println!("Task ID is required");
    }
    Ok(())
}

/// Freezes an account using sudo privileges.
///
/// # Arguments
///
/// * `_sub_m` - A reference to the ArgMatches struct containing parsed command-line arguments.
///
/// # Returns
///
/// A Result containing () if successful, or a Box<dyn std::error::Error> if an error occurs.
pub async fn sudo_freeze_account(_sub_m: &clap::ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_to_gevulot(_sub_m).await?;
    if let Some(account) = _sub_m.get_one::<String>("account") {
        let msg = MsgSudoFreezeAccount {
            authority: client.base_client.write().await.address.clone().unwrap(),
            account: account.clone(),
        };
        client.sudo.freeze_account(msg).await?;
        println!("{}", OK);
    } else {
        println!("Account address is required");
    }
    Ok(())
}
