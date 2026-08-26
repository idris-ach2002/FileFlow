use crate::{
    ComponentKind, PlanStep, PlannedOperation, SetupMode, SetupPlan, SetupProfile, SetupRequest,
    SystemSnapshot,
};
use chrono::Utc;
use serde_json::json;
use std::collections::HashSet;
use uuid::Uuid;

pub fn build_plan(snapshot: &SystemSnapshot, request: SetupRequest) -> SetupPlan {
    let mut steps = Vec::new();
    let mut warnings = snapshot.warnings.clone();

    push_step(
        &mut steps,
        "preflight",
        "Diagnostic système",
        "Vérification de la plateforme, de l’architecture, des permissions et des processus actifs.",
        ComponentKind::Verification,
        PlannedOperation::Inspect,
        5,
        true,
        false,
        "Aucune modification n’a encore été effectuée.",
        json!({}),
    );

    match request.mode {
        SetupMode::Install | SetupMode::Repair => {
            plan_install_or_repair(snapshot, &request, &mut steps, &mut warnings)
        }
        SetupMode::Uninstall => plan_uninstall(snapshot, &request, &mut steps, &mut warnings),
        SetupMode::Doctor => {
            push_step(
                &mut steps,
                "doctor",
                "Diagnostic FileFlow",
                "Contrôle de l’application, des moteurs et de l’intégration sans modifier la machine.",
                ComponentKind::Verification,
                PlannedOperation::Verify,
                90,
                true,
                false,
                "Aucune restauration nécessaire : le diagnostic est en lecture seule.",
                json!({ "engineCount": snapshot.engines.len() }),
            );
        }
    }

    let total_weight = steps.iter().map(|step| step.weight).sum();
    SetupPlan {
        operation_id: Uuid::new_v4(),
        created_at: Utc::now(),
        request,
        platform: snapshot.platform,
        architecture: snapshot.architecture,
        steps,
        warnings,
        total_weight,
    }
}

fn plan_install_or_repair(
    snapshot: &SystemSnapshot,
    request: &SetupRequest,
    steps: &mut Vec<PlanStep>,
    warnings: &mut Vec<String>,
) {
    if request.profile == SetupProfile::EnginesOnly {
        plan_engines(snapshot, request, steps, warnings, true);
        return;
    }

    let differential_repair = request.mode == SetupMode::Repair
        && snapshot.application.installed
        && snapshot.application.version.is_some()
        && !snapshot.integration.healthy();

    if differential_repair {
        push_step(
            steps,
            "release",
            "Vérification",
            "Contrôle de l’installation existante sans retélécharger FileFlow inutilement.",
            ComponentKind::Application,
            PlannedOperation::Preserve,
            2,
            true,
            false,
            "Aucune modification de l’application.",
            json!({ "differentialRepair": true }),
        );
        push_step(
            steps,
            "application",
            "FileFlow",
            "L’application existante est conservée ; le contrôle final décidera si une réinstallation complète est nécessaire.",
            ComponentKind::Application,
            PlannedOperation::Preserve,
            2,
            true,
            false,
            "Aucune modification de l’application.",
            json!({ "installed": true, "differentialRepair": true }),
        );
    } else {
        push_step(
            steps,
            "release",
            "Vérification",
            "Téléchargement du manifeste public puis validation du paquet, du checksum et de l’architecture.",
            ComponentKind::Application,
            PlannedOperation::Download,
            10,
            true,
            false,
            "Le fichier temporaire est supprimé et l’installation existante est conservée.",
            json!({}),
        );

        push_step(
            steps,
            "application",
            "FileFlow",
            "Installation en zone temporaire, contrôle du bundle puis activation atomique.",
            ComponentKind::Application,
            if snapshot.application.installed {
                PlannedOperation::Repair
            } else {
                PlannedOperation::Install
            },
            35,
            false,
            cfg!(target_os = "windows"),
            "La version précédente est restaurée automatiquement.",
            json!({ "installed": snapshot.application.installed }),
        );
    }

    if snapshot.application.installed || !differential_repair {
        push_step(
            steps,
            "integration",
            "Intégration système",
            match snapshot.platform {
                crate::Platform::Macos => {
                    "Vérification de l’application et de son icône dans macOS."
                }
                crate::Platform::Windows => {
                    "Vérification ou recréation du raccourci FileFlow dans le menu Démarrer avec l’icône de l’application."
                }
                crate::Platform::Linux => {
                    "Vérification ou recréation du lanceur, du wrapper et de l’icône FileFlow dans le menu Applications."
                }
            },
            ComponentKind::Integration,
            if snapshot.integration.healthy() {
                PlannedOperation::Preserve
            } else {
                PlannedOperation::Repair
            },
            if snapshot.integration.healthy() { 2 } else { 8 },
            true,
            false,
            "L’intégration précédente reste récupérable depuis l’application installée.",
            json!({
                "launcherInstalled": snapshot.integration.launcher_installed,
                "iconInstalled": snapshot.integration.icon_installed,
            }),
        );
    }

    plan_engines(snapshot, request, steps, warnings, false);

    push_step(
        steps,
        "maintenance",
        "Maintenance",
        "Copie vérifiée de FileFlow Setup pour permettre réparation et désinstallation ultérieures.",
        ComponentKind::Maintenance,
        PlannedOperation::Install,
        8,
        true,
        false,
        "La copie de maintenance précédente est restaurée.",
        json!({}),
    );
    push_step(
        steps,
        "postcheck",
        "Contrôle final",
        "Validation de FileFlow, de l’intégration système, de l’IPC, des moteurs et de la version installée.",
        ComponentKind::Verification,
        PlannedOperation::Verify,
        15,
        true,
        false,
        "L’installation est marquée incomplète et la restauration est proposée.",
        json!({}),
    );
    push_step(
        steps,
        "receipt",
        "Diagnostic d’installation",
        "Enregistrement des composants, de l’intégration système et des moteurs réellement présents.",
        ComponentKind::Maintenance,
        PlannedOperation::WriteReceipt,
        2,
        true,
        false,
        "Le journal transactionnel permet de reconstruire le reçu.",
        json!({}),
    );
    if request.launch_after {
        push_step(
            steps,
            "launch",
            "Ouvrir FileFlow",
            "Ouverture de FileFlow après validation complète de l’installation.",
            ComponentKind::Application,
            PlannedOperation::Finalize,
            3,
            true,
            false,
            "L’application peut être relancée manuellement.",
            json!({}),
        );
    }

    if request.profile == SetupProfile::ApplicationOnly {
        warnings.push("Les moteurs locaux ne seront pas installés avec ce profil.".into());
    }
    if differential_repair {
        warnings.push("Réparation différentielle : FileFlow et les moteurs valides sont conservés ; seule l’intégration système manquante est recréée.".into());
    }
}

fn plan_engines(
    snapshot: &SystemSnapshot,
    request: &SetupRequest,
    steps: &mut Vec<PlanStep>,
    warnings: &mut Vec<String>,
    engines_only: bool,
) {
    let selected: HashSet<&str> = request
        .selected_engines
        .iter()
        .map(String::as_str)
        .collect();
    let known = snapshot
        .engines
        .iter()
        .map(|engine| engine.id.as_str())
        .collect::<HashSet<_>>();
    let unknown = selected.difference(&known).copied().collect::<Vec<_>>();
    if !unknown.is_empty() {
        warnings.push(format!(
            "Moteurs inconnus ignorés : {}.",
            unknown.join(", ")
        ));
    }
    let install_all =
        request.profile == SetupProfile::Standard || (engines_only && selected.is_empty());
    let missing = snapshot
        .engines
        .iter()
        .filter(|engine| {
            !engine.installed
                && request.profile != SetupProfile::ApplicationOnly
                && (install_all || selected.contains(engine.id.as_str()))
        })
        .map(|engine| engine.id.clone())
        .collect::<Vec<_>>();
    let requested = snapshot
        .engines
        .iter()
        .filter(|engine| install_all || selected.contains(engine.id.as_str()))
        .map(|engine| engine.id.clone())
        .collect::<Vec<_>>();

    if !missing.is_empty() {
        push_step(
            steps,
            "engines",
            "Moteurs locaux",
            "Installation indépendante des moteurs manquants avec le gestionnaire natif.",
            ComponentKind::Engine,
            PlannedOperation::Install,
            25,
            true,
            true,
            "Seuls les moteurs installés pendant cette opération sont proposés au retrait.",
            json!({ "missing": missing, "requested": requested.clone() }),
        );
    } else {
        push_step(
            steps,
            "engines",
            "Moteurs locaux",
            "Les moteurs sélectionnés sont déjà disponibles ou ont été volontairement ignorés.",
            ComponentKind::Engine,
            PlannedOperation::Preserve,
            2,
            true,
            false,
            "Aucune modification des moteurs.",
            json!({ "requested": requested.clone() }),
        );
    }
    if engines_only {
        push_step(
            steps,
            "engine-postcheck",
            "Vérification des moteurs",
            "Nouveau diagnostic limité aux moteurs sélectionnés, sans démarrer FileFlow.",
            ComponentKind::Verification,
            PlannedOperation::Verify,
            8,
            true,
            false,
            "Aucune modification supplémentaire pendant cette vérification.",
            json!({ "requested": requested }),
        );
        push_step(
            steps,
            "receipt",
            "Reçu des moteurs",
            "Enregistrement des moteurs ajoutés par FileFlow Setup pour une désinstallation sûre.",
            ComponentKind::Maintenance,
            PlannedOperation::WriteReceipt,
            2,
            true,
            false,
            "Le journal transactionnel permet de reconstruire le reçu.",
            json!({ "enginesOnly": true }),
        );
    }
}

fn plan_uninstall(
    snapshot: &SystemSnapshot,
    request: &SetupRequest,
    steps: &mut Vec<PlanStep>,
    warnings: &mut Vec<String>,
) {
    push_step(
        steps,
        "stop",
        "Arrêt propre",
        "Arrêt de FileFlow et de ses processus enfants après vérification des traitements actifs.",
        ComponentKind::Application,
        PlannedOperation::Stop,
        10,
        true,
        false,
        "FileFlow peut être relancé tant qu’aucun fichier n’a été déplacé.",
        json!({ "running": snapshot.application.running }),
    );
    push_step(
        steps,
        "remove-application",
        "Retrait atomique",
        "Déplacement de l’application vers une quarantaine récupérable avant suppression définitive.",
        ComponentKind::Application,
        PlannedOperation::Remove,
        40,
        false,
        cfg!(target_os = "windows"),
        "Le bundle est restauré depuis la quarantaine.",
        json!({ "path": snapshot.application.path }),
    );

    if request.remove_owned_engines || request.remove_preexisting_engines {
        let owned = if request.remove_owned_engines {
            snapshot
                .receipt
                .as_ref()
                .into_iter()
                .flat_map(|receipt| receipt.components.iter())
                .filter(|component| {
                    component.installed_by_fileflow
                        && matches!(
                            component.kind,
                            ComponentKind::Engine | ComponentKind::Integration
                        )
                })
                .map(|component| component.id.clone())
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let preexisting = if request.remove_preexisting_engines {
            snapshot
                .engines
                .iter()
                .filter(|engine| {
                    engine.installed
                        && !engine.installed_by_fileflow
                        && request.selected_engines.iter().any(|id| id == &engine.id)
                })
                .map(|engine| engine.id.clone())
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        if !preexisting.is_empty() {
            warnings.push(format!(
                "Mode expert : {} moteur(s) préexistant(s) seront retirés. Cela peut affecter d’autres applications.",
                preexisting.len()
            ));
        }
        push_step(
            steps,
            "remove-engines",
            if preexisting.is_empty() {
                "Moteurs possédés par FileFlow"
            } else {
                "Moteurs et bibliothèques sélectionnés"
            },
            if preexisting.is_empty() {
                "Retrait uniquement des moteurs dont le reçu prouve l’installation par FileFlow."
            } else {
                "Retrait des moteurs FileFlow et des moteurs préexistants explicitement sélectionnés en mode expert."
            },
            ComponentKind::Engine,
            PlannedOperation::Remove,
            25,
            true,
            true,
            "Réinstallation manuelle peut être nécessaire pour les dépendances préexistantes retirées en mode expert.",
            json!({ "owned": owned, "preexisting": preexisting }),
        );
    }

    if request.remove_cache || request.remove_settings || request.remove_history {
        push_step(
            steps,
            "remove-data",
            "Données facultatives",
            "Suppression limitée aux catégories explicitement confirmées.",
            ComponentKind::Cache,
            PlannedOperation::Remove,
            10,
            true,
            false,
            "Les éléments récupérables sont placés en quarantaine lorsque le système le permet.",
            json!({
                "cache": request.remove_cache,
                "settings": request.remove_settings,
                "history": request.remove_history,
                "outputs": false
            }),
        );
    }

    push_step(
        steps,
        "uninstall-postcheck",
        "Post-contrôle",
        "Vérification de l’absence du bundle, des processus et des intégrations sélectionnées.",
        ComponentKind::Verification,
        PlannedOperation::Verify,
        10,
        true,
        false,
        "Les restes sont expliqués et peuvent être supprimés ou conservés.",
        json!({}),
    );
    push_step(
        steps,
        "uninstall-report",
        "Rapport final",
        "Résumé des éléments retirés, conservés et récupérables.",
        ComponentKind::Maintenance,
        PlannedOperation::Finalize,
        5,
        true,
        false,
        "La quarantaine reste disponible tant que le rapport n’est pas validé.",
        json!({ "preserveOutputs": true }),
    );

    if !request.preserve_outputs {
        warnings.push(
            "Les fichiers produits restent protégés : leur suppression n’est jamais automatisée."
                .into(),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn push_step(
    steps: &mut Vec<PlanStep>,
    id: &str,
    title: &str,
    description: &str,
    component: ComponentKind,
    operation: PlannedOperation,
    weight: u32,
    interruptible: bool,
    requires_elevation: bool,
    rollback_description: &str,
    metadata: serde_json::Value,
) {
    steps.push(PlanStep {
        id: id.into(),
        title: title.into(),
        description: description.into(),
        component,
        operation,
        weight,
        interruptible,
        requires_elevation,
        rollback_description: rollback_description.into(),
        metadata,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ApplicationState, Architecture, EngineState, InstallReceipt, IntegrationState, Platform,
        ReceiptComponent,
    };
    use chrono::Utc;
    use std::path::PathBuf;

    fn snapshot() -> SystemSnapshot {
        SystemSnapshot {
            platform: Platform::Macos,
            architecture: Architecture::Aarch64,
            application: ApplicationState {
                installed: true,
                version: Some("1.0.6".into()),
                path: Some(PathBuf::from("/Applications/FileFlow.app")),
                running: false,
            },
            integration: IntegrationState {
                launcher_installed: true,
                icon_installed: true,
                maintenance_installed: true,
            },
            engines: vec![
                EngineState {
                    id: "ffmpeg".into(),
                    label: "FFmpeg".into(),
                    installed: true,
                    executable: Some(PathBuf::from("/opt/homebrew/bin/ffmpeg")),
                    version: None,
                    installed_by_fileflow: false,
                },
                EngineState {
                    id: "zstd".into(),
                    label: "Zstandard".into(),
                    installed: false,
                    executable: None,
                    version: None,
                    installed_by_fileflow: false,
                },
            ],
            receipt_path: PathBuf::from("receipt.json"),
            receipt: None,
            warnings: vec![],
        }
    }

    #[test]
    fn standard_install_only_plans_missing_engines() {
        let plan = build_plan(&snapshot(), SetupRequest::default());
        let engines = plan.steps.iter().find(|step| step.id == "engines").unwrap();
        assert_eq!(engines.operation, PlannedOperation::Install);
        assert_eq!(engines.metadata["missing"], json!(["zstd"]));
    }

    #[test]
    fn app_only_never_installs_engines() {
        let request = SetupRequest {
            profile: SetupProfile::ApplicationOnly,
            ..SetupRequest::default()
        };
        let plan = build_plan(&snapshot(), request);
        let engines = plan.steps.iter().find(|step| step.id == "engines").unwrap();
        assert_eq!(engines.operation, PlannedOperation::Preserve);
    }

    #[test]
    fn custom_profile_installs_only_selected_missing_engines() {
        let empty = build_plan(
            &snapshot(),
            SetupRequest {
                profile: SetupProfile::Custom,
                ..SetupRequest::default()
            },
        );
        assert_eq!(
            empty
                .steps
                .iter()
                .find(|step| step.id == "engines")
                .unwrap()
                .operation,
            PlannedOperation::Preserve
        );

        let selected = build_plan(
            &snapshot(),
            SetupRequest {
                profile: SetupProfile::Custom,
                selected_engines: vec!["zstd".into()],
                ..SetupRequest::default()
            },
        );
        assert_eq!(
            selected
                .steps
                .iter()
                .find(|step| step.id == "engines")
                .unwrap()
                .metadata["missing"],
            json!(["zstd"])
        );
    }

    #[test]
    fn repair_only_recreates_missing_system_integration() {
        let mut observed = snapshot();
        observed.integration.launcher_installed = false;
        observed.integration.icon_installed = false;
        let plan = build_plan(
            &observed,
            SetupRequest {
                mode: SetupMode::Repair,
                ..SetupRequest::default()
            },
        );
        assert_eq!(
            plan.steps
                .iter()
                .find(|step| step.id == "application")
                .unwrap()
                .operation,
            PlannedOperation::Preserve
        );
        assert_eq!(
            plan.steps
                .iter()
                .find(|step| step.id == "integration")
                .unwrap()
                .operation,
            PlannedOperation::Repair
        );
    }

    #[test]
    fn engines_only_never_downloads_or_reinstalls_the_application() {
        let plan = build_plan(
            &snapshot(),
            SetupRequest {
                profile: SetupProfile::EnginesOnly,
                selected_engines: vec!["zstd".into()],
                launch_after: false,
                ..SetupRequest::default()
            },
        );
        assert!(plan.steps.iter().all(|step| step.id != "release"));
        assert!(plan.steps.iter().all(|step| step.id != "application"));
        assert!(plan.steps.iter().all(|step| step.id != "maintenance"));
        assert!(plan.steps.iter().all(|step| step.id != "launch"));
        assert_eq!(
            plan.steps
                .iter()
                .find(|step| step.id == "engines")
                .unwrap()
                .metadata["missing"],
            json!(["zstd"])
        );
        assert!(plan.steps.iter().any(|step| step.id == "engine-postcheck"));
    }

    #[test]
    fn engines_only_defaults_to_every_missing_engine() {
        let plan = build_plan(
            &snapshot(),
            SetupRequest {
                profile: SetupProfile::EnginesOnly,
                launch_after: false,
                ..SetupRequest::default()
            },
        );
        assert_eq!(
            plan.steps
                .iter()
                .find(|step| step.id == "engines")
                .unwrap()
                .metadata["missing"],
            json!(["zstd"])
        );
    }

    #[test]
    fn uninstall_never_plans_output_removal() {
        let request = SetupRequest {
            mode: SetupMode::Uninstall,
            profile: SetupProfile::FullRemoval,
            remove_owned_engines: true,
            remove_settings: true,
            remove_history: true,
            remove_cache: true,
            preserve_outputs: false,
            ..SetupRequest::default()
        };
        let plan = build_plan(&snapshot(), request);
        let data = plan
            .steps
            .iter()
            .find(|step| step.id == "remove-data")
            .unwrap();
        assert_eq!(data.metadata["outputs"], false);
        assert!(
            plan.warnings
                .iter()
                .any(|warning| warning.contains("protégés"))
        );
    }

    #[test]
    fn preexisting_engine_removal_is_explicit_and_warned() {
        let plan = build_plan(
            &snapshot(),
            SetupRequest {
                mode: SetupMode::Uninstall,
                selected_engines: vec!["ffmpeg".into()],
                remove_owned_engines: false,
                remove_preexisting_engines: true,
                ..SetupRequest::default()
            },
        );
        let engines = plan
            .steps
            .iter()
            .find(|step| step.id == "remove-engines")
            .unwrap();
        assert_eq!(engines.metadata["preexisting"], json!(["ffmpeg"]));
        assert_eq!(engines.metadata["owned"], json!([]));
        assert!(
            plan.warnings
                .iter()
                .any(|warning| warning.contains("Mode expert"))
        );
    }

    #[test]
    fn expert_only_uninstall_never_claims_owned_components() {
        let mut observed = snapshot();
        observed.receipt = Some(InstallReceipt {
            schema_version: 2,
            operation_id: Uuid::new_v4(),
            installed_at: Utc::now(),
            application_version: "1.0.7".into(),
            platform: Platform::Macos,
            architecture: Architecture::Aarch64,
            components: vec![ReceiptComponent {
                id: "zstd".into(),
                kind: ComponentKind::Engine,
                version: None,
                path: None,
                installed_by_fileflow: true,
                package_manager: Some("brew".into()),
                packages: vec!["zstd".into()],
                checksum: None,
                rollback_hint: Some("brew uninstall zstd".into()),
            }],
            outputs_are_user_owned: true,
        });
        observed.engines[1].installed = true;
        observed.engines[1].installed_by_fileflow = true;
        let plan = build_plan(
            &observed,
            SetupRequest {
                mode: SetupMode::Uninstall,
                selected_engines: vec!["ffmpeg".into()],
                remove_owned_engines: false,
                remove_preexisting_engines: true,
                ..SetupRequest::default()
            },
        );
        let engines = plan
            .steps
            .iter()
            .find(|step| step.id == "remove-engines")
            .unwrap();
        assert_eq!(engines.metadata["owned"], json!([]));
        assert_eq!(engines.metadata["preexisting"], json!(["ffmpeg"]));
    }

    #[test]
    fn owned_engine_information_comes_from_receipt() {
        let mut observed = snapshot();
        observed.receipt = Some(InstallReceipt {
            schema_version: 1,
            operation_id: Uuid::new_v4(),
            installed_at: Utc::now(),
            application_version: "1.0.6".into(),
            platform: Platform::Macos,
            architecture: Architecture::Aarch64,
            components: vec![ReceiptComponent {
                id: "zstd".into(),
                kind: ComponentKind::Engine,
                version: None,
                path: None,
                installed_by_fileflow: true,
                package_manager: Some("brew".into()),
                packages: vec!["zstd".into()],
                checksum: None,
                rollback_hint: Some("brew uninstall zstd".into()),
            }],
            outputs_are_user_owned: true,
        });
        observed.engines[1].installed = true;
        observed.engines[1].installed_by_fileflow = true;
        let request = SetupRequest {
            mode: SetupMode::Uninstall,
            remove_owned_engines: true,
            ..SetupRequest::default()
        };
        let plan = build_plan(&observed, request);
        let engines = plan
            .steps
            .iter()
            .find(|step| step.id == "remove-engines")
            .unwrap();
        assert_eq!(engines.metadata["owned"], json!(["zstd"]));
    }
}
