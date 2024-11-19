use gevulot_rs::builders::{ByteSize, ByteUnit, MsgCreatePinBuilder, MsgDeletePinBuilder};

use crate::{connect_to_gevulot, print_object, read_file};

/// Lists all pins in the Gevulot network
///
/// This function connects to the Gevulot network, retrieves all pins,
/// and prints them as YAML output.
///
/// # Arguments
///
/// * `_sub_m` - A reference to the ArgMatches struct containing parsed command-line arguments.
///              This is used to access any additional flags or options passed to the command.
pub async fn list_pins(_sub_m: &clap::ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_to_gevulot(_sub_m).await?;
    let pins = client.pins.list().await?;
    // Convert the pins to the gevulot_rs::models::Pin type
    let pins: Vec<gevulot_rs::models::Pin> = pins.into_iter().map(Into::into).collect();
    print_object(_sub_m, &pins)?;
    Ok(())
}

/// Retrieves a specific pin from the Gevulot network
///
/// This function connects to the Gevulot network, retrieves a pin by its CID,
/// and prints it as YAML output.
///
/// # Arguments
///
/// * `_sub_m` - A reference to the ArgMatches struct containing parsed command-line arguments.
///              This is used to access the CID of the pin to retrieve and any additional options.
pub async fn get_pin(_sub_m: &clap::ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = connect_to_gevulot(_sub_m).await?;
    if let Some(pin_cid) = _sub_m.get_one::<String>("cid") {
        let pin = client.pins.get(pin_cid).await?;
        // Convert the pin to the gevulot_rs::models::Pin type
        let pin: gevulot_rs::models::Pin = pin.into();
        print_object(_sub_m, &pin)?;
    } else {
        println!("Pin CID is required");
    }
    Ok(())
}

/// Creates a new pin in the Gevulot network
///
/// This function reads pin data from a file, connects to the Gevulot network,
/// and creates a new pin with the provided data.
///
/// # Arguments
///
/// * `_sub_m` - A reference to the ArgMatches struct containing parsed command-line arguments.
///              This is used to access the file path for pin data and any additional options.
pub async fn create_pin(_sub_m: &clap::ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    // Read pin data from file
    let pin: gevulot_rs::models::Pin = read_file(_sub_m).await?;
    let mut client = connect_to_gevulot(_sub_m).await?;

    // Get the client's address
    let me = client
        .base_client
        .write()
        .await
        .address
        .clone()
        .ok_or("No address found, did you set a mnemonic?")?;

    // Create the pin using the MsgCreatePinBuilder
    client
        .pins
        .create(
            MsgCreatePinBuilder::default()
                .creator(me.clone())
                .cid(pin.spec.cid.clone())
                .fallback_urls(pin.spec.fallback_urls.unwrap_or_default())
                .bytes(ByteSize::new(pin.spec.bytes as u64, ByteUnit::Byte))
                .time(pin.spec.time as u64)
                .redundancy(pin.spec.redundancy as u64)
                .name(pin.metadata.name)
                .description(pin.metadata.description)
                .labels(pin.metadata.labels.into_iter().map(Into::into).collect())
                .tags(pin.metadata.tags)
                .into_message()?,
        )
        .await?;

    // Replace println with print_object for consistent formatting
    print_object(_sub_m, &serde_json::json!({
        "status": "success",
        "message": format!("Created pin with CID: {}", &pin.spec.cid)
    }))?;
    Ok(())
}

/// Deletes a pin from the Gevulot network
///
/// This function connects to the Gevulot network and deletes a pin specified by its CID.
///
/// # Arguments
///
/// * `_sub_m` - A reference to the ArgMatches struct containing parsed command-line arguments.
///              This is used to access the CID of the pin to delete and any additional options.
pub async fn delete_pin(_sub_m: &clap::ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    let pin_cid = _sub_m
        .get_one::<String>("cid")
        .ok_or("Pin CID is required")?;
    let mut client = connect_to_gevulot(_sub_m).await?;

    // Get the client's address
    let me = client
        .base_client
        .write()
        .await
        .address
        .clone()
        .ok_or("No address found, did you set a mnemonic?")?;

    // Delete the pin using the MsgDeletePinBuilder
    client
        .pins
        .delete(
            MsgDeletePinBuilder::default()
                .creator(me.clone())
                .cid(pin_cid.clone())
                .into_message()?,
        )
        .await?;

    // Use print_object for consistent formatting
    print_object(_sub_m, &serde_json::json!({
        "status": "success",
        "message": format!("Deleted pin with CID: {}", pin_cid)
    }))?;
    Ok(())
}
