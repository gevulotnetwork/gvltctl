use gevulot_rs::proto::gevulot::gevulot::{workflow_spec::Stage, InputContext, MsgCreateWorkflow, MsgDeleteWorkflow, OutputContext, TaskEnv, TaskSpec, WorkflowSpec};

use crate::{connect_to_gevulot, print_object, read_file};




pub async fn list_workflows(_sub_m: &clap::ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    let mut client = crate::connect_to_gevulot(_sub_m).await?;
    let workflows = client.workflows.list().await?;
    let workflows: Vec<gevulot_rs::models::Workflow> = workflows.into_iter().map(Into::into).collect();
    print_object(_sub_m, &workflows)?;
    Ok(())
}

pub async fn get_workflow(_sub_m: &clap::ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(workflow_id) = _sub_m.get_one::<String>("id") {
        let mut client = crate::connect_to_gevulot(_sub_m).await?;
        let workflow = client.workflows.get(workflow_id).await?;
        let workflow: gevulot_rs::models::Workflow = workflow.into();
        print_object(_sub_m, &workflow)?;
    } else {
        println!("Workflow ID is required");
    }
    Ok(())
}

pub async fn create_workflow(_sub_m: &clap::ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    let workflow: gevulot_rs::models::WorkflowSpec = read_file(_sub_m).await?;
    let mut client = connect_to_gevulot(_sub_m).await?;
    let me = client
        .base_client
        .write()
        .await
        .address
        .clone()
        .ok_or("No address found, did you set a mnemonic?")?;

    let resp = client
        .workflows
        .create(MsgCreateWorkflow{
            creator: me,
            spec: Some(WorkflowSpec{
                stages: workflow.stages.iter().map(|s| Stage{
                    tasks: s.tasks.iter().map(|t| TaskSpec{
                        image: t.image.clone(),
                        command: t.command.clone(),
                        args: t.args.clone(),
                        env: t.env.iter().map(|e| TaskEnv{
                            name: e.name.clone(),
                            value: e.value.clone()
                        }).collect(),
                        input_contexts: t.input_contexts.iter().map(|ic| InputContext{
                            source: ic.source.clone(),
                            target: ic.target.clone()
                        }).collect(),
                        output_contexts: t.output_contexts.iter().map(|oc| OutputContext{
                            source: oc.source.clone(),
                            retention_period: oc.retention_period as u64
                        }).collect(),
                        cpus: t.resources.cpus as u64,
                        gpus: t.resources.gpus as u64,
                        memory: t.resources.memory as u64,
                        time: t.resources.time as u64,
                        store_stdout: t.store_stdout.unwrap_or(false),
                        store_stderr: t.store_stderr.unwrap_or(false),
                        workflow_ref: "".to_string(),
                    }).collect::<Vec<TaskSpec>>(),
                }).collect::<Vec<Stage>>(),
            }),
        }).await?;

    println!("Created workflow with ID: {}", resp.id);
    Ok(())
}

pub async fn delete_workflow(_sub_m: &clap::ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(workflow_id) = _sub_m.get_one::<String>("id") {
        let mut client = crate::connect_to_gevulot(_sub_m).await?;
        let me = client
            .base_client
            .write()
            .await
            .address
            .clone()
            .ok_or("No address found, did you set a mnemonic?")?;

        client.workflows.delete(MsgDeleteWorkflow{
            creator: me,
            id: workflow_id.clone(),
        }).await?;
        println!("ok");
    } else {
        println!("Workflow ID is required");
    }
    Ok(())
}
