use clap::{Arg, Command, ValueHint};
use gevulot_rs::proto::gevulot::gevulot::{
    MsgSudoDeletePin, MsgSudoDeleteTask, MsgSudoDeleteWorker, MsgSudoFreezeAccount,
};

use crate::{connect_to_gevulot, print_object};

pub fn get_command(chain_args: &[Arg]) -> clap::Command {
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
                )
                .args(chain_args),
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
                )
                .args(chain_args),
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
                )
                .args(chain_args),
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
                )
                .args(chain_args),
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
        print_object(_sub_m, &serde_json::json!({
            "status": "success",
            "message": "Pin deleted successfully"
        }))?;
    } else {
        print_object(_sub_m, &serde_json::json!({
            "status": "error",
            "message": "Pin ID is required"
        }))?;
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
        print_object(_sub_m, &serde_json::json!({
            "status": "success",
            "message": "Worker deleted successfully"
        }))?;
    } else {
        print_object(_sub_m, &serde_json::json!({
            "status": "error",
            "message": "Worker ID is required"
        }))?;
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
        print_object(_sub_m, &serde_json::json!({
            "status": "success",
            "message": "Task deleted successfully"
        }))?;
    } else {
        print_object(_sub_m, &serde_json::json!({
            "status": "error",
            "message": "Task ID is required"
        }))?;
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
        print_object(_sub_m, &serde_json::json!({
            "status": "success",
            "message": "Account frozen successfully"
        }))?;
    } else {
        print_object(_sub_m, &serde_json::json!({
            "status": "error",
            "message": "Account address is required"
        }))?;
    }
    Ok(())
}
