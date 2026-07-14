use std::time::Duration;

use anyhow::{ensure, Context};
use edgion_center_adapter_kubernetes::{
    controller_resource_name, EdgionController, KubernetesControllerDirectory,
    KubernetesLeaseCoordinator,
};
use edgion_center_core::{
    ControllerDirectory, ControllerId, ControllerRegistration, CoordinationRole, Coordinator,
    OwnershipFence, ReleaseOutcome, SessionId,
};
use k8s_openapi::api::coordination::v1::Lease;
use kube::{
    api::{DeleteParams, ListParams},
    Api, Client,
};
use uuid::Uuid;

/// Real kube-apiserver matrix. It is opt-in because it creates namespaced CRD
/// and Lease resources. The target namespace and EdgionController CRD must
/// already exist; the test never installs cluster-scoped resources.
#[tokio::test]
async fn reconstruction_and_lease_takeover_survive_replica_restart() {
    if std::env::var("EDGION_TEST_KUBERNETES").as_deref() != Ok("1") {
        eprintln!("skipping: EDGION_TEST_KUBERNETES=1 is not set");
        return;
    }
    let namespace = std::env::var("EDGION_TEST_KUBERNETES_NAMESPACE")
        .expect("EDGION_TEST_KUBERNETES_NAMESPACE must name a disposable namespace");
    let client = Client::try_default().await.expect("Kubernetes client");
    let suffix = Uuid::new_v4().simple().to_string();
    let controller_id = ControllerId::new(format!("integration/{suffix}")).unwrap();

    let result = run_matrix(client.clone(), &namespace, &suffix, &controller_id).await;
    let cleanup = cleanup(client, &namespace, &controller_id).await;
    if let Err(error) = cleanup {
        if result.is_ok() {
            panic!("integration cleanup failed: {error:#}");
        }
        eprintln!("cleanup after integration failure also failed: {error:#}");
    }
    result.unwrap_or_else(|error| panic!("real kube-apiserver matrix failed: {error:#}"));
}

async fn run_matrix(
    client: Client,
    namespace: &str,
    suffix: &str,
    controller_id: &ControllerId,
) -> anyhow::Result<()> {
    let session_1 = SessionId::new(format!("session-1-{suffix}"))?;
    let session_2 = SessionId::new(format!("session-2-{suffix}"))?;
    let expected_connected = format!("center-a/{suffix}");
    let directory_a = KubernetesControllerDirectory::new(client.clone(), namespace);
    directory_a
        .upsert_registration(registration(
            controller_id,
            &session_1,
            &expected_connected,
            1,
            suffix,
        ))
        .await
        .context("project first registration")?;

    let controllers: Api<EdgionController> = Api::namespaced(client.clone(), namespace);
    let resource_name = controller_resource_name(controller_id.as_str());
    let first = controllers
        .get(&resource_name)
        .await
        .context("read first CRD projection")?;
    let generation = first.metadata.generation.context("CRD generation")?;
    let first_rv = first
        .metadata
        .resource_version
        .clone()
        .context("first status resourceVersion")?;
    let first_status = first.status.context("first CRD status")?;
    ensure!(first_status.observed_generation == Some(generation));
    ensure!(first_status.session_id.as_deref() == Some(session_1.as_str()));

    directory_a
        .upsert_registration(registration(
            controller_id,
            &session_2,
            &expected_connected,
            2,
            suffix,
        ))
        .await
        .context("project second registration")?;
    let second = controllers
        .get(&resource_name)
        .await
        .context("read second CRD projection")?;
    ensure!(second.metadata.generation == Some(generation));
    ensure!(second.metadata.resource_version.as_deref() != Some(first_rv.as_str()));
    let second_status = second.status.context("second CRD status")?;
    ensure!(second_status.observed_generation == Some(generation));
    ensure!(second_status.session_id.as_deref() == Some(session_2.as_str()));
    ensure!(second_status.ownership_epoch == 2);

    // A fresh adapter has no process-local cache and must reconstruct the row
    // entirely from the Kubernetes API, matching a Center replica restart.
    let directory_b = KubernetesControllerDirectory::new(client.clone(), namespace);
    let records = directory_b.list().await.context("reconstruct directory")?;
    let reconstructed = records
        .iter()
        .find(|record| record.controller_id == *controller_id)
        .context("projected controller is visible after restart")?;
    ensure!(reconstructed.current_session_id.as_ref() == Some(&session_2));
    ensure!(reconstructed.connected_replica.as_deref() == Some(expected_connected.as_str()));

    let coordinator_a = KubernetesLeaseCoordinator::new(
        client.clone(),
        namespace,
        format!("center-a/{suffix}"),
        Duration::from_secs(5),
    )?;
    let holder_b = format!("center-b/{suffix}");
    let coordinator_b = KubernetesLeaseCoordinator::new(
        client,
        namespace,
        holder_b.clone(),
        Duration::from_secs(5),
    )?;
    let role = CoordinationRole::ControllerOwner(controller_id.to_string());
    let first_owner = coordinator_a.acquire(role.clone()).await?;
    ensure!(coordinator_b.acquire(role.clone()).await.is_err());
    let takeover = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            match coordinator_b.acquire(role.clone()).await {
                Ok(leadership) => break Ok(leadership),
                Err(edgion_center_core::CoreError::Conflict(_)) => {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
                Err(error) => break Err(error),
            }
        }
    })
    .await
    .context("takeover remained blocked after Lease expiry")??;
    ensure!(takeover.holder == holder_b);
    ensure!(takeover.fencing_epoch > first_owner.fencing_epoch);
    ensure!(coordinator_a.release(&first_owner).await? == ReleaseOutcome::Lost);
    coordinator_b.release(&takeover).await?;
    Ok(())
}

fn registration(
    controller_id: &ControllerId,
    session_id: &SessionId,
    connected_replica: &str,
    epoch: u64,
    suffix: &str,
) -> ControllerRegistration {
    ControllerRegistration {
        controller_id: controller_id.clone(),
        session_id: session_id.clone(),
        cluster: "integration".to_string(),
        environments: vec!["e2e".to_string()],
        tags: vec!["real-apiserver".to_string()],
        connected_replica: Some(connected_replica.to_string()),
        ownership_fence: Some(OwnershipFence {
            token: format!("token-{epoch}-{suffix}"),
            epoch,
        }),
        observed_at_unix_ms: epoch as i64,
    }
}

async fn cleanup(
    client: Client,
    namespace: &str,
    controller_id: &ControllerId,
) -> anyhow::Result<()> {
    let mut errors = Vec::new();
    let controllers: Api<EdgionController> = Api::namespaced(client.clone(), namespace);
    if let Err(error) = controllers
        .delete(
            &controller_resource_name(controller_id.as_str()),
            &DeleteParams::default(),
        )
        .await
    {
        if !matches!(&error, kube::Error::Api(response) if response.code == 404) {
            errors.push(format!("delete controller projection: {error}"));
        }
    }

    let leases: Api<Lease> = Api::namespaced(client, namespace);
    let role_key = format!("controller-owner:{controller_id}");
    match leases.list(&ListParams::default()).await {
        Ok(list) => {
            for lease in list.items {
                let managed =
                    lease.metadata.annotations.as_ref().and_then(|annotations| {
                        annotations.get("center.edgion.io/coordination-role")
                    }) == Some(&role_key);
                if managed {
                    if let Some(name) = lease.metadata.name {
                        if let Err(error) = leases.delete(&name, &DeleteParams::default()).await {
                            errors.push(format!("delete Lease {name}: {error}"));
                        }
                    }
                }
            }
        }
        Err(error) => errors.push(format!("list Leases for cleanup: {error}")),
    }
    if !errors.is_empty() {
        anyhow::bail!(errors.join("; "));
    }
    Ok(())
}
