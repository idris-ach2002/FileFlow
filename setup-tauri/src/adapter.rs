use async_trait::async_trait;
use chrono::Utc;
#[cfg(debug_assertions)]
use fileflow_setup_core::sha256_file;
use fileflow_setup_core::{
    ActionOutcome, ComponentKind, DownloadArtifact, DownloadManifest, EventLevel, EventSink,
    InstallReceipt, PlanStep, PlannedOperation, Platform, ReceiptComponent, SetupActionAdapter,
    SetupEvent, SetupMode, SetupPlan, SystemSnapshot, application_candidates, probe_system,
    verify_artifact, write_receipt,
};
use serde_json::json;
use std::{
    collections::HashSet,
    ffi::OsStr,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    fs,
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    process::Command,
};

const DOWNLOAD_ENDPOINT: &str =
    "https://github.com/idris-ach2002/FileFlow/releases/latest/download/downloads.json";

pub async fn latest_setup_version() -> Result<String, String> {
    let endpoint =
        std::env::var("FILEFLOW_DOWNLOAD_ENDPOINT").unwrap_or_else(|_| DOWNLOAD_ENDPOINT.into());
    if !endpoint.starts_with("https://") {
        return Err("endpoint de mise à jour Setup non HTTPS".into());
    }
    let mut command = Command::new(curl_program());
    command.args([
        "--fail",
        "--location",
        "--silent",
        "--show-error",
        "--connect-timeout",
        "5",
        "--max-time",
        "20",
        "--retry",
        "2",
        "--retry-delay",
        "1",
        "--retry-all-errors",
        &endpoint,
    ]);
    hide_process(&mut command);
    isolate_process(&mut command);
    let output = command.output().await.map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    let manifest: DownloadManifest = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("downloads.json invalide: {error}"))?;
    manifest.validate().map_err(|error| error.to_string())?;
    Ok(manifest.version)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InstalledPackage {
    component_id: String,
    manager: String,
    package: String,
    integration: bool,
}

#[derive(Default)]
struct AdapterState {
    manifest: Option<DownloadManifest>,
    application_artifact: Option<DownloadArtifact>,
    downloaded_application: Option<PathBuf>,
    installed_application: Option<PathBuf>,
    application_backup: Option<PathBuf>,
    uninstall_quarantine: Option<PathBuf>,
    maintenance_path: Option<PathBuf>,
    maintenance_backup: Option<PathBuf>,
    maintenance_changed: bool,
    engines_installed: Vec<String>,
    packages_installed: Vec<InstalledPackage>,
    packages_preserved: Vec<String>,
    data_quarantine: Vec<(PathBuf, PathBuf)>,
    database_backups: Vec<(PathBuf, PathBuf)>,
}

pub struct SystemSetupAdapter {
    resource_dir: PathBuf,
    operation_dir: PathBuf,
    initial: SystemSnapshot,
    state: Mutex<AdapterState>,
    sequence: AtomicU64,
}

impl SystemSetupAdapter {
    pub fn new(resource_dir: PathBuf, operation_dir: PathBuf, initial: SystemSnapshot) -> Self {
        Self {
            resource_dir,
            operation_dir,
            initial,
            state: Mutex::new(AdapterState::default()),
            sequence: AtomicU64::new(10_000),
        }
    }

    async fn fetch_release(
        &self,
        plan: &SetupPlan,
        events: &dyn EventSink,
        cancellation: &AtomicBool,
    ) -> Result<(), String> {
        fs::create_dir_all(&self.operation_dir)
            .await
            .map_err(|error| error.to_string())?;
        #[cfg(debug_assertions)]
        if let Some(package) = std::env::var_os("FILEFLOW_SETUP_LOCAL_APPLICATION") {
            let package = fs::canonicalize(PathBuf::from(package))
                .await
                .map_err(|error| format!("paquet local introuvable: {error}"))?;

            let metadata = fs::metadata(&package)
                .await
                .map_err(|error| error.to_string())?;

            if !metadata.is_file() || metadata.len() == 0 {
                return Err("le paquet local doit être un fichier non vide".into());
            }

            let extension = package
                .extension()
                .and_then(OsStr::to_str)
                .unwrap_or_default();

            let package_type = match plan.platform {
                Platform::Macos if extension.eq_ignore_ascii_case("dmg") => "dmg",
                Platform::Windows if extension.eq_ignore_ascii_case("exe") => "exe",
                Platform::Linux if extension.eq_ignore_ascii_case("appimage") => "appimage",
                _ => {
                    return Err("le paquet local ne correspond pas à la plateforme".into());
                }
            };

            let version = std::env::var("FILEFLOW_SETUP_LOCAL_VERSION")
                .unwrap_or_else(|_| env!("CARGO_PKG_VERSION").into());

            if let Some(installed) = self.initial.application.version.as_deref()
                && version_tuple(installed) > version_tuple(&version)
            {
                return Err(format!("downgrade refusé: {installed} vers {version}"));
            }

            let name = package
                .file_name()
                .and_then(OsStr::to_str)
                .ok_or_else(|| "nom du paquet local invalide".to_string())?
                .to_owned();

            let artifact = DownloadArtifact {
                name: name.clone(),
                url: format!("local-debug://{name}"),
                sha256: sha256_file(&package).map_err(|error| error.to_string())?,
                size: metadata.len(),
                signature: None,
                package_type: package_type.into(),
                role: Some("application".into()),
                target: None,
            };

            let manifest = DownloadManifest {
                schema_version: 1,
                version,
                published_at: Utc::now().to_rfc3339(),
                repository: "local-debug".into(),
                platforms: Default::default(),
            };

            self.emit(
                events,
                plan,
                "artifact-local-debug",
                EventLevel::Warning,
                Some("release"),
                "Mode développement : paquet local vérifié par SHA-256",
                Some(metadata.len()),
                Some(metadata.len()),
                Some("bytes"),
                json!({
                    "name": name,
                    "path": package.display().to_string()
                }),
            );

            let mut state = self.state.lock().map_err(|error| error.to_string())?;

            state.application_artifact = Some(artifact);
            state.downloaded_application = Some(package);
            state.manifest = Some(manifest);

            return Ok(());
        }
        let endpoint = std::env::var("FILEFLOW_DOWNLOAD_ENDPOINT")
            .unwrap_or_else(|_| DOWNLOAD_ENDPOINT.into());
        let manifest_path = self.operation_dir.join("downloads.json");
        self.download(
            plan,
            "release",
            &endpoint,
            &manifest_path,
            None,
            events,
            cancellation,
        )
        .await?;
        let manifest: DownloadManifest = serde_json::from_slice(
            &fs::read(&manifest_path)
                .await
                .map_err(|error| error.to_string())?,
        )
        .map_err(|error| format!("downloads.json invalide: {error}"))?;
        manifest.validate().map_err(|error| error.to_string())?;
        if let Some(installed) = self.initial.application.version.as_deref()
            && version_tuple(installed) > version_tuple(&manifest.version)
        {
            return Err(format!(
                "FileFlow {installed} est plus récent que la release stable {} ; le downgrade est refusé",
                manifest.version
            ));
        }
        let downloads = manifest
            .for_current_platform(plan.platform, plan.architecture)
            .map_err(|error| error.to_string())?;
        let artifact = downloads.application.clone();
        let destination = self.operation_dir.join(&artifact.name);
        self.download(
            plan,
            "release",
            &artifact.url,
            &destination,
            Some(artifact.size),
            events,
            cancellation,
        )
        .await?;
        verify_artifact(&destination, &artifact).map_err(|error| error.to_string())?;
        self.emit(
            events,
            plan,
            "artifact-verified",
            EventLevel::Success,
            Some("release"),
            "Taille et SHA-256 du paquet validés depuis le manifeste de release",
            Some(artifact.size),
            Some(artifact.size),
            Some("bytes"),
            json!({ "name": artifact.name, "sha256": artifact.sha256 }),
        );
        let mut state = self.state.lock().map_err(|error| error.to_string())?;
        state.application_artifact = Some(artifact);
        state.downloaded_application = Some(destination);
        state.manifest = Some(manifest);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn download(
        &self,
        plan: &SetupPlan,
        step_id: &str,
        url: &str,
        destination: &Path,
        expected_size: Option<u64>,
        events: &dyn EventSink,
        cancellation: &AtomicBool,
    ) -> Result<(), String> {
        if !url.starts_with("https://") {
            return Err("FileFlow refuse un téléchargement non HTTPS".into());
        }
        let manifest_request = url.ends_with("/downloads.json");
        let partial = destination.with_extension("part");
        let mut command = Command::new(curl_program());
        command
            .args([
                "--fail",
                "--location",
                "--silent",
                "--show-error",
                "--connect-timeout",
                if manifest_request { "5" } else { "15" },
                "--max-time",
                if manifest_request { "20" } else { "900" },
            ])
            .arg("--continue-at")
            .arg("-")
            .arg("--output")
            .arg(&partial)
            .arg(url)
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        if !manifest_request {
            command.args(["--retry", "3", "--retry-delay", "2", "--retry-all-errors"]);
        }
        hide_process(&mut command);
        isolate_process(&mut command);
        let mut child = command
            .spawn()
            .map_err(|error| format!("curl ne peut pas démarrer: {error}"))?;
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        let deadline = tokio::time::Instant::now() + Duration::from_secs(15 * 60);
        loop {
            interval.tick().await;
            if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
                if !status.success() {
                    let mut details = String::new();
                    if let Some(mut stderr) = child.stderr.take() {
                        let _ = stderr.read_to_string(&mut details).await;
                    }
                    let details = redact_line(details.trim());
                    if manifest_request
                        && (details.contains("404")
                            || details.to_ascii_lowercase().contains("not found"))
                    {
                        return Err(
                            "aucune release FileFlow Setup installable n’est encore publiée (downloads.json retourne HTTP 404)".into(),
                        );
                    }
                    return Err(if details.is_empty() {
                        format!("téléchargement impossible depuis {url}")
                    } else {
                        format!("téléchargement impossible depuis {url}: {details}")
                    });
                }
                break;
            }
            if tokio::time::Instant::now() >= deadline {
                terminate_process_tree(&mut child).await;
                return Err("le téléchargement a dépassé la limite de 15 minutes".into());
            }
            if cancellation.load(Ordering::Relaxed) {
                terminate_process_tree(&mut child).await;
                return Err("téléchargement annulé".into());
            }
            if !manifest_request {
                let completed = fs::metadata(&partial)
                    .await
                    .map(|value| value.len())
                    .unwrap_or(0);
                self.emit(
                    events,
                    plan,
                    "bytes-progress",
                    EventLevel::Info,
                    Some(step_id),
                    "Téléchargement en cours",
                    Some(completed),
                    expected_size,
                    Some("bytes"),
                    json!({ "url": url }),
                );
            }
        }
        fs::rename(&partial, destination)
            .await
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    fn verify_selected_engines(
        &self,
        plan: &SetupPlan,
        step: &PlanStep,
        events: &dyn EventSink,
    ) -> Result<(), String> {
        let requested = step
            .metadata
            .get("requested")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .collect::<HashSet<_>>();
        let observed = probe_system().map_err(|error| error.to_string())?;
        let unavailable = observed
            .engines
            .iter()
            .filter(|engine| requested.contains(engine.id.as_str()) && !engine.installed)
            .map(|engine| engine.label.clone())
            .collect::<Vec<_>>();
        if !unavailable.is_empty() {
            return Err(format!(
                "moteurs toujours indisponibles après installation : {}",
                unavailable.join(", ")
            ));
        }
        self.emit(
            events,
            plan,
            "engines-verified",
            EventLevel::Success,
            Some("engine-postcheck"),
            &format!("{} moteur(s) sélectionné(s) vérifié(s)", requested.len()),
            Some(requested.len() as u64),
            Some(requested.len() as u64),
            Some("engines"),
            json!({ "requested": requested }),
        );
        Ok(())
    }

    async fn install_application(
        &self,
        plan: &SetupPlan,
        events: &dyn EventSink,
    ) -> Result<(), String> {
        let package = self
            .state
            .lock()
            .map_err(|error| error.to_string())?
            .downloaded_application
            .clone()
            .ok_or_else(|| "le paquet FileFlow vérifié est absent".to_string())?;
        match plan.platform {
            Platform::Macos => self.install_macos(plan, events, &package).await,
            Platform::Linux => self.install_linux(plan, events, &package).await,
            Platform::Windows => self.install_windows(plan, events, &package).await,
        }
    }

    async fn install_macos(
        &self,
        plan: &SetupPlan,
        events: &dyn EventSink,
        package: &Path,
    ) -> Result<(), String> {
        let mount = self.operation_dir.join("mount");
        fs::create_dir_all(&mount)
            .await
            .map_err(|error| error.to_string())?;
        run_checked(
            "hdiutil",
            &[
                OsStr::new("attach"),
                package.as_os_str(),
                OsStr::new("-nobrowse"),
                OsStr::new("-readonly"),
                OsStr::new("-mountpoint"),
                mount.as_os_str(),
            ],
        )
        .await?;
        let installation = async {
            let source = find_named_directory(&mount, "FileFlow.app", 3)
                .ok_or_else(|| "FileFlow.app est absent du DMG".to_string())?;
            run_checked(
                "codesign",
                &[
                    OsStr::new("--verify"),
                    OsStr::new("--deep"),
                    OsStr::new("--strict"),
                    source.as_os_str(),
                ],
            )
            .await?;

            let destination = if is_writable(Path::new("/Applications")) {
                PathBuf::from("/Applications/FileFlow.app")
            } else {
                home_dir().join("Applications").join("FileFlow.app")
            };
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)
                    .await
                    .map_err(|error| error.to_string())?;
            }
            let stage = self.operation_dir.join("FileFlow.app.installing");
            let backup = self.operation_dir.join("FileFlow.app.backup");
            remove_path_if_exists(&stage).await?;
            remove_path_if_exists(&backup).await?;
            run_checked("ditto", &[source.as_os_str(), stage.as_os_str()]).await?;
            if destination.exists() {
                fs::rename(&destination, &backup)
                    .await
                    .map_err(|error| format!("sauvegarde de l’application impossible: {error}"))?;
            }
            if let Err(error) = fs::rename(&stage, &destination).await {
                if backup.exists() {
                    let _ = fs::rename(&backup, &destination).await;
                }
                return Err(format!("activation de FileFlow impossible: {error}"));
            }
            Ok::<_, String>((destination, backup))
        }
        .await;
        let detach = run_checked("hdiutil", &[OsStr::new("detach"), mount.as_os_str()]).await;
        let (destination, backup) = installation?;
        if let Err(error) = detach {
            self.emit(
                events,
                plan,
                "dmg-detach-warning",
                EventLevel::Warning,
                Some("application"),
                &format!(
                    "FileFlow est installé, mais le DMG devra être éjecté manuellement: {error}"
                ),
                None,
                None,
                None,
                json!({}),
            );
        }
        let mut state = self.state.lock().map_err(|error| error.to_string())?;
        state.installed_application = Some(destination.clone());
        state.application_backup = backup.exists().then_some(backup);
        drop(state);
        self.emit(
            events,
            plan,
            "application-activated",
            EventLevel::Success,
            Some("application"),
            "FileFlow.app a été activé atomiquement",
            None,
            None,
            None,
            json!({ "path": destination }),
        );
        Ok(())
    }

    async fn install_linux(
        &self,
        plan: &SetupPlan,
        events: &dyn EventSink,
        package: &Path,
    ) -> Result<(), String> {
        let app_dir = home_dir().join(".local").join("opt").join("fileflow");
        let destination = app_dir.join("FileFlow.AppImage");
        fs::create_dir_all(&app_dir)
            .await
            .map_err(|error| error.to_string())?;
        let stage = app_dir.join(".FileFlow.AppImage.installing");
        let backup = self.operation_dir.join("FileFlow.AppImage.backup");
        fs::copy(package, &stage)
            .await
            .map_err(|error| error.to_string())?;
        run_checked("chmod", &[OsStr::new("+x"), stage.as_os_str()]).await?;
        if destination.exists() {
            fs::rename(&destination, &backup)
                .await
                .map_err(|error| error.to_string())?;
        }
        if let Err(error) = fs::rename(&stage, &destination).await {
            if backup.exists() {
                let _ = fs::rename(&backup, &destination).await;
            }
            return Err(error.to_string());
        }
        if let Err(error) = install_linux_integration(&destination, &self.resource_dir).await {
            let _ = remove_path_if_exists(&destination).await;
            if backup.exists() {
                let _ = fs::rename(&backup, &destination).await;
            }
            return Err(format!("intégration Linux impossible: {error}"));
        }
        let mut state = self.state.lock().map_err(|error| error.to_string())?;
        state.installed_application = Some(destination.clone());
        state.application_backup = backup.exists().then_some(backup);
        drop(state);
        self.emit(
            events,
            plan,
            "application-activated",
            EventLevel::Success,
            Some("application"),
            "FileFlow AppImage et ses intégrations ont été installés",
            None,
            None,
            None,
            json!({ "path": destination }),
        );
        Ok(())
    }

    async fn install_windows(
        &self,
        plan: &SetupPlan,
        events: &dyn EventSink,
        package: &Path,
    ) -> Result<(), String> {
        let backup = self.operation_dir.join("FileFlow.windows.backup");
        remove_path_if_exists(&backup).await?;
        if let Some(existing) = self
            .initial
            .application
            .path
            .as_deref()
            .and_then(Path::parent)
            .filter(|path| path.is_dir())
        {
            run_windows_powershell(
                "$ErrorActionPreference='Stop'; Copy-Item -LiteralPath $env:FILEFLOW_PS_SOURCE -Destination $env:FILEFLOW_PS_DESTINATION -Recurse -Force",
                &[
                    ("FILEFLOW_PS_SOURCE", existing.as_os_str()),
                    ("FILEFLOW_PS_DESTINATION", backup.as_os_str()),
                ],
            )
            .await?;
        }
        let natively_signed = verify_windows_signature(package).await?;
        if !natively_signed {
            self.emit(
                events,
                plan,
                "native-signature-warning",
                EventLevel::Warning,
                Some("application"),
                "Le paquet Windows n’a pas de signature Authenticode ; le SHA-256 de la release a toutefois été vérifié",
                None,
                None,
                None,
                json!({}),
            );
        }
        let extension = package
            .extension()
            .and_then(OsStr::to_str)
            .unwrap_or_default()
            .to_ascii_lowercase();
        if extension == "msi" {
            run_checked(
                "msiexec.exe",
                &[
                    OsStr::new("/i"),
                    package.as_os_str(),
                    OsStr::new("/passive"),
                    OsStr::new("/norestart"),
                ],
            )
            .await?;
        } else {
            run_checked(package.as_os_str(), &[OsStr::new("/S")]).await?;
        }
        let installed = application_candidates(Platform::Windows)
            .into_iter()
            .find(|path| path.exists())
            .ok_or_else(|| {
                "l’installateur Windows a terminé sans application détectable".to_string()
            })?;
        install_windows_integration(&installed).await?;
        let mut state = self.state.lock().map_err(|error| error.to_string())?;
        state.installed_application = Some(installed.clone());
        state.application_backup = backup.exists().then_some(backup);
        drop(state);
        self.emit(
            events,
            plan,
            "application-activated",
            EventLevel::Success,
            Some("application"),
            "FileFlow a été enregistré dans Windows",
            None,
            None,
            None,
            json!({ "path": installed }),
        );
        Ok(())
    }

    async fn ensure_system_integration(
        &self,
        plan: &SetupPlan,
        events: &dyn EventSink,
    ) -> Result<(), String> {
        let observed = probe_system().map_err(|error| error.to_string())?;
        if observed.integration.healthy() {
            self.emit(
                events,
                plan,
                "integration-ready",
                EventLevel::Success,
                Some("integration"),
                "Raccourci et icône FileFlow déjà valides",
                None,
                None,
                None,
                json!({ "platform": plan.platform, "changed": false }),
            );
            return Ok(());
        }

        let application = self
            .state
            .lock()
            .map_err(|error| error.to_string())?
            .installed_application
            .clone()
            .or_else(|| self.initial.application.path.clone())
            .or_else(|| {
                application_candidates(plan.platform)
                    .into_iter()
                    .find(|path| path.exists())
            })
            .ok_or_else(|| {
                "FileFlow doit être installé avant de réparer son intégration système".to_string()
            })?;

        match plan.platform {
            Platform::Linux => install_linux_integration(&application, &self.resource_dir).await?,
            Platform::Windows => install_windows_integration(&application).await?,
            Platform::Macos => {
                if !application.exists() {
                    return Err("FileFlow.app est absent de macOS".into());
                }
            }
        }

        self.emit(
            events,
            plan,
            "integration-ready",
            EventLevel::Success,
            Some("integration"),
            "Raccourci et icône FileFlow vérifiés",
            None,
            None,
            None,
            json!({ "platform": plan.platform, "changed": true }),
        );
        Ok(())
    }

    async fn install_engines(
        &self,
        plan: &SetupPlan,
        step: &PlanStep,
        events: &dyn EventSink,
        cancellation: &AtomicBool,
    ) -> Result<(), String> {
        if step.operation == PlannedOperation::Preserve {
            return Ok(());
        }
        let script_name = if cfg!(target_os = "windows") {
            "install-dependencies.ps1"
        } else {
            "install-dependencies.sh"
        };
        let script = find_resource(&self.resource_dir, script_name)
            .or_else(|| find_resource(Path::new("."), script_name))
            .ok_or_else(|| format!("ressource {script_name} absente"))?;
        fs::create_dir_all(&self.operation_dir)
            .await
            .map_err(|error| error.to_string())?;
        let ownership_report = self.operation_dir.join("installed-packages.tsv");
        remove_path_if_exists(&ownership_report).await?;
        let mut command = if cfg!(target_os = "windows") {
            let mut value = Command::new(windows_powershell_program());
            value.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]);
            value.arg(&script);
            value
        } else if cfg!(target_os = "linux")
            && !is_root()
            && command_exists("pkexec")
            && std::env::var_os("CI").is_none()
            && std::env::var_os("GITHUB_ACTIONS").is_none()
        {
            let mut value = Command::new("pkexec");
            value.arg("bash").arg(&script);
            value
        } else {
            let mut value = Command::new("bash");
            value.arg(&script);
            value
        };
        command.env(
            "FILEFLOW_SETUP_INSTALL_APP_RUNTIME",
            if plan.request.profile == fileflow_setup_core::SetupProfile::EnginesOnly {
                "0"
            } else {
                "1"
            },
        );
        command.env(
            "FILEFLOW_SETUP_INSTALL_SUPPORT_TOOLS",
            if cfg!(target_os = "windows")
                && plan.request.profile != fileflow_setup_core::SetupProfile::EnginesOnly
            {
                "1"
            } else {
                "0"
            },
        );
        command.env("FILEFLOW_SETUP_ENGINE_REPORT", &ownership_report);
        if cfg!(target_os = "windows") {
            command.arg("-ReportPath").arg(&ownership_report);
        } else {
            command.arg("--report").arg(&ownership_report);
        }
        if let Some(missing) = step
            .metadata
            .get("missing")
            .and_then(serde_json::Value::as_array)
        {
            let selected = missing
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect::<Vec<_>>()
                .join(",");
            command.env("FILEFLOW_SETUP_ENGINES", &selected);
            if cfg!(target_os = "windows") {
                command.arg("-Engines").arg(selected);
            } else {
                command.arg("--engines").arg(selected);
            }
        }
        self.run_streaming(plan, "engines", &mut command, events, cancellation)
            .await?;

        let missing_before: HashSet<_> = self
            .initial
            .engines
            .iter()
            .filter(|engine| !engine.installed)
            .map(|engine| engine.id.clone())
            .collect();
        let after = probe_system().map_err(|error| error.to_string())?;
        let installed = after
            .engines
            .iter()
            .filter(|engine| engine.installed && missing_before.contains(&engine.id))
            .map(|engine| engine.id.clone())
            .collect::<Vec<_>>();
        let installed_set = installed.iter().map(String::as_str).collect::<HashSet<_>>();
        let packages = read_package_report(&ownership_report)?
            .into_iter()
            .filter(|record| {
                record.integration || installed_set.contains(record.component_id.as_str())
            })
            .collect::<Vec<_>>();
        let mut state = self.state.lock().map_err(|error| error.to_string())?;
        state.engines_installed = installed;
        state.packages_installed = packages;
        Ok(())
    }

    async fn persist_maintenance(&self) -> Result<(), String> {
        let current = std::env::current_exe().map_err(|error| error.to_string())?;
        let parent = self
            .initial
            .receipt_path
            .parent()
            .ok_or_else(|| "dossier de maintenance introuvable".to_string())?;
        let maintenance = parent.join("maintenance");
        fs::create_dir_all(&maintenance)
            .await
            .map_err(|error| error.to_string())?;
        let (source, destination, directory) = match self.initial.platform {
            Platform::Macos => {
                let bundle = current
                    .ancestors()
                    .find(|path| path.extension().and_then(OsStr::to_str) == Some("app"));
                if let Some(bundle) = bundle {
                    (
                        bundle.to_owned(),
                        maintenance.join("FileFlowSetup.app"),
                        true,
                    )
                } else {
                    (current.clone(), maintenance.join("fileflow-setup"), false)
                }
            }
            Platform::Windows => (current, maintenance.join("FileFlowSetup.exe"), false),
            Platform::Linux => {
                let source = std::env::var_os("APPIMAGE")
                    .map(PathBuf::from)
                    .unwrap_or(current);
                (source, maintenance.join("FileFlowSetup.AppImage"), false)
            }
        };
        let maintenance_changed = source != destination && !source.starts_with(&destination);
        let backup = if maintenance_changed {
            Some(
                replace_maintenance(
                    &source,
                    &destination,
                    directory,
                    &self.operation_dir.join("maintenance.backup"),
                )
                .await?,
            )
            .filter(|path| path.exists())
        } else {
            None
        };
        if self.initial.platform == Platform::Linux {
            run_checked("chmod", &[OsStr::new("+x"), destination.as_os_str()]).await?;
        }
        let mut state = self.state.lock().map_err(|error| error.to_string())?;
        state.maintenance_path = Some(destination);
        state.maintenance_backup = backup;
        state.maintenance_changed = maintenance_changed;
        Ok(())
    }

    async fn postcheck(
        &self,
        plan: &SetupPlan,
        events: &dyn EventSink,
        cancellation: &AtomicBool,
    ) -> Result<(), String> {
        let snapshot = probe_system().map_err(|error| error.to_string())?;
        match plan.request.mode {
            SetupMode::Install | SetupMode::Repair => {
                if !snapshot.application.installed {
                    return Err("FileFlow est absent après l’installation".into());
                }
                if !snapshot.integration.healthy() {
                    return Err(format!(
                        "intégration système incomplète après installation (raccourci={}, icône={})",
                        snapshot.integration.launcher_installed,
                        snapshot.integration.icon_installed
                    ));
                }
                self.emit(
                    events,
                    plan,
                    "integration-verified",
                    EventLevel::Success,
                    Some("postcheck"),
                    "FileFlow est visible dans le lanceur système avec son icône",
                    None,
                    None,
                    None,
                    json!({
                        "launcher": snapshot.integration.launcher_installed,
                        "icon": snapshot.integration.icon_installed,
                    }),
                );
                self.smoke_installed_application(plan, events, cancellation)
                    .await?;
                if let Some(script) = find_resource(&self.resource_dir, doctor_script_name())
                    .or_else(|| find_resource(Path::new("."), doctor_script_name()))
                {
                    let mut command = doctor_command(&script);
                    let status = command.status().await.map_err(|error| error.to_string())?;
                    if !status.success() {
                        self.emit(
                            events,
                            plan,
                            "doctor-warning",
                            EventLevel::Warning,
                            Some("postcheck"),
                            "L’application fonctionne, mais certains moteurs restent indisponibles",
                            None,
                            None,
                            None,
                            json!({}),
                        );
                    }
                }
            }
            SetupMode::Uninstall => {
                if snapshot.application.installed {
                    return Err("FileFlow est encore détecté après la désinstallation".into());
                }
            }
            SetupMode::Doctor => {}
        }
        Ok(())
    }

    async fn smoke_installed_application(
        &self,
        plan: &SetupPlan,
        events: &dyn EventSink,
        cancellation: &AtomicBool,
    ) -> Result<(), String> {
        let application = self
            .state
            .lock()
            .map_err(|error| error.to_string())?
            .installed_application
            .clone()
            .or_else(|| self.initial.application.path.clone())
            .ok_or_else(|| "application installée introuvable pour le post-contrôle".to_string())?;
        let expected_version = self
            .state
            .lock()
            .map_err(|error| error.to_string())?
            .manifest
            .as_ref()
            .map(|manifest| manifest.version.clone())
            .or_else(|| self.initial.application.version.clone())
            .ok_or_else(|| {
                "version attendue absente du manifeste et de l’installation existante".to_string()
            })?;
        let executable = application_executable(plan.platform, &application)?;
        let health_dir =
            std::env::temp_dir().join(format!("fileflow-setup-health-{}", plan.operation_id));
        remove_path_if_exists(&health_dir).await?;
        fs::create_dir_all(&health_dir)
            .await
            .map_err(|error| error.to_string())?;
        let health_file = health_dir.join("health.json");
        let mut command = Command::new(&executable);
        command
            .current_dir(executable.parent().unwrap_or_else(|| Path::new(".")))
            .env("FILEFLOW_SMOKE_TEST", "1")
            .env("FILEFLOW_SMOKE_HEALTH_FILE", &health_file)
            .env("APPIMAGE_EXTRACT_AND_RUN", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        hide_process(&mut command);
        isolate_process(&mut command);
        let mut child = command
            .spawn()
            .map_err(|error| format!("post-contrôle FileFlow impossible à démarrer: {error}"))?;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(45);
        let validation = loop {
            if cancellation.load(Ordering::Relaxed) {
                break Err("post-contrôle annulé".into());
            }
            if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
                break Err(format!(
                    "FileFlow s’est fermé avant le handshake de post-contrôle ({status})"
                ));
            }
            if let Ok(bytes) = fs::read(&health_file).await
                && let Ok(payload) = serde_json::from_slice::<serde_json::Value>(&bytes)
            {
                let valid = payload.get("backend").and_then(serde_json::Value::as_bool)
                    == Some(true)
                    && payload.get("frontend").and_then(serde_json::Value::as_bool) == Some(true)
                    && payload
                        .pointer("/health/app")
                        .and_then(serde_json::Value::as_str)
                        == Some("FileFlow")
                    && payload
                        .pointer("/health/version")
                        .and_then(serde_json::Value::as_str)
                        == Some(expected_version.as_str());
                if valid {
                    break Ok(payload);
                }
            }
            if tokio::time::Instant::now() >= deadline {
                break Err("le handshake Angular → Tauri du post-contrôle a expiré".into());
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        };
        terminate_process_tree(&mut child).await;
        let _ = remove_path_if_exists(&health_dir).await;
        let payload = validation?;
        self.emit(
            events,
            plan,
            "application-health-verified",
            EventLevel::Success,
            Some("postcheck"),
            "FileFlow a démarré hors écran et le handshake Angular → Tauri a réussi",
            None,
            None,
            None,
            json!({
                "version": payload.pointer("/health/version"),
                "platform": payload.pointer("/health/os"),
                "architecture": payload.pointer("/health/architecture"),
            }),
        );
        Ok(())
    }

    async fn write_install_receipt(&self, plan: &SetupPlan) -> Result<(), String> {
        let observed = probe_system().map_err(|error| error.to_string())?;
        let (receipt, backup) = {
            let state = self.state.lock().map_err(|error| error.to_string())?;
            let version = state
                .manifest
                .as_ref()
                .map(|manifest| manifest.version.clone())
                .or_else(|| self.initial.application.version.clone())
                .unwrap_or_else(|| env!("CARGO_PKG_VERSION").into());
            let application_path = state
                .installed_application
                .clone()
                .or_else(|| self.initial.application.path.clone());
            let app_checksum = state
                .application_artifact
                .as_ref()
                .map(|artifact| artifact.sha256.clone());
            let previous_component = |id: &str| {
                self.initial
                    .receipt
                    .as_ref()
                    .and_then(|receipt| receipt.components.iter().find(|item| item.id == id))
            };
            let mut components = Vec::new();
            if let Some(path) = application_path.clone() {
                components.push(ReceiptComponent {
                    id: "fileflow".into(),
                    kind: ComponentKind::Application,
                    version: Some(version.clone()),
                    path: Some(path),
                    installed_by_fileflow: state.installed_application.is_some()
                        || previous_component("fileflow")
                            .is_some_and(|item| item.installed_by_fileflow),
                    package_manager: None,
                    packages: Vec::new(),
                    checksum: app_checksum.or_else(|| {
                        previous_component("fileflow").and_then(|item| item.checksum.clone())
                    }),
                    rollback_hint: Some("restore-application-backup".into()),
                });
            }
            let maintenance_path = state.maintenance_path.clone().or_else(|| {
                previous_component("fileflow-setup").and_then(|item| item.path.clone())
            });
            if let Some(path) = maintenance_path {
                components.push(ReceiptComponent {
                    id: "fileflow-setup".into(),
                    kind: ComponentKind::Maintenance,
                    version: Some(version.clone()),
                    path: Some(path),
                    installed_by_fileflow: state.maintenance_path.is_some()
                        || previous_component("fileflow-setup")
                            .is_some_and(|item| item.installed_by_fileflow),
                    package_manager: None,
                    packages: Vec::new(),
                    checksum: None,
                    rollback_hint: None,
                });
            }
            if observed.integration.healthy() {
                components.push(ReceiptComponent {
                    id: "fileflow-system-integration".into(),
                    kind: ComponentKind::Integration,
                    version: Some(version.clone()),
                    path: match plan.platform {
                        Platform::Linux => {
                            Some(home_dir().join(".local/share/applications/fileflow.desktop"))
                        }
                        Platform::Windows => windows_shortcut_candidates()
                            .into_iter()
                            .find(|path| path.exists()),
                        Platform::Macos => application_path.clone(),
                    },
                    installed_by_fileflow: true,
                    package_manager: None,
                    packages: Vec::new(),
                    checksum: None,
                    rollback_hint: Some("rebuild-system-integration".into()),
                });
            }
            for engine in &observed.engines {
                let installed_package = state
                    .packages_installed
                    .iter()
                    .find(|record| !record.integration && record.component_id == engine.id);
                let previous = previous_component(&engine.id);
                components.push(ReceiptComponent {
                    id: engine.id.clone(),
                    kind: ComponentKind::Engine,
                    version: engine.version.clone(),
                    path: engine.executable.clone(),
                    installed_by_fileflow: state.engines_installed.contains(&engine.id)
                        || previous.is_some_and(|item| item.installed_by_fileflow),
                    package_manager: installed_package
                        .map(|record| record.manager.clone())
                        .or_else(|| previous.and_then(|item| item.package_manager.clone())),
                    packages: installed_package
                        .map(|record| vec![record.package.clone()])
                        .or_else(|| previous.map(|item| item.packages.clone()))
                        .unwrap_or_default(),
                    checksum: None,
                    rollback_hint: None,
                });
            }
            for record in state
                .packages_installed
                .iter()
                .filter(|record| record.integration)
            {
                components.push(ReceiptComponent {
                    id: record.component_id.clone(),
                    kind: ComponentKind::Integration,
                    version: None,
                    path: None,
                    installed_by_fileflow: true,
                    package_manager: Some(record.manager.clone()),
                    packages: vec![record.package.clone()],
                    checksum: None,
                    rollback_hint: None,
                });
            }
            if let Some(previous_receipt) = self.initial.receipt.as_ref() {
                let missing_integrations = previous_receipt
                    .components
                    .iter()
                    .filter(|component| {
                        component.installed_by_fileflow
                            && component.kind == ComponentKind::Integration
                            && !components.iter().any(|item| item.id == component.id)
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                components.extend(missing_integrations);
            }
            (
                InstallReceipt {
                    schema_version: 2,
                    operation_id: plan.operation_id,
                    installed_at: Utc::now(),
                    application_version: version,
                    platform: plan.platform,
                    architecture: plan.architecture,
                    components,
                    outputs_are_user_owned: true,
                },
                state.application_backup.clone(),
            )
        };
        write_receipt(&self.initial.receipt_path, &receipt).map_err(|error| error.to_string())?;
        if let Some(backup) = backup {
            remove_path_if_exists(&backup).await?;
        }
        Ok(())
    }

    async fn launch_application(&self) -> Result<(), String> {
        let application = self
            .state
            .lock()
            .map_err(|error| error.to_string())?
            .installed_application
            .clone()
            .or_else(|| self.initial.application.path.clone())
            .ok_or_else(|| "FileFlow installé introuvable".to_string())?;
        let mut command = if self.initial.platform == Platform::Macos {
            let mut value = Command::new("open");
            value.arg(&application);
            value
        } else {
            Command::new(&application)
        };
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        hide_process(&mut command);
        command
            .spawn()
            .map_err(|error| format!("FileFlow ne peut pas démarrer: {error}"))?;
        Ok(())
    }

    async fn stop_application(&self) -> Result<(), String> {
        if self.initial.platform == Platform::Windows {
            for image_name in ["FileFlow.exe", "fileflow-desktop.exe"] {
                let mut command = Command::new("taskkill.exe");
                command.args(["/IM", image_name, "/T", "/F"]);
                hide_process(&mut command);
                let _ = command.status().await;
            }
        } else {
            let mut command = Command::new("pkill");
            command.args(["-f", "fileflow-desktop"]);
            hide_process(&mut command);
            let _ = command.status().await;
        }

        for _ in 0..10 {
            tokio::time::sleep(Duration::from_millis(200)).await;
            let running = probe_system()
                .map_err(|error| error.to_string())?
                .application
                .running;
            if !running {
                return Ok(());
            }
        }
        Err("FileFlow n’a pas pu être fermé complètement".into())
    }

    async fn quarantine_application(&self) -> Result<(), String> {
        let Some(path) = self.initial.application.path.as_ref() else {
            return Ok(());
        };
        let source = if self.initial.platform == Platform::Windows {
            path.parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| path.clone())
        } else {
            path.clone()
        };
        if !source.exists() {
            return Ok(());
        }
        let quarantine = self.operation_dir.join("uninstall-quarantine");
        remove_path_if_exists(&quarantine).await?;
        fs::rename(&source, &quarantine)
            .await
            .map_err(|error| format!("mise en quarantaine impossible: {error}"))?;
        self.state
            .lock()
            .map_err(|error| error.to_string())?
            .uninstall_quarantine = Some(quarantine);
        Ok(())
    }

    async fn remove_owned_engines(
        &self,
        plan: &SetupPlan,
        events: &dyn EventSink,
    ) -> Result<(), String> {
        let owned = if plan.request.remove_owned_engines {
            self.initial
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
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let mut removed_packages = HashSet::new();
        for component in owned {
            let removals = removal_commands(plan.platform, component);
            if removals.is_empty() {
                self.state
                    .lock()
                    .map_err(|error| error.to_string())?
                    .packages_preserved
                    .push(component.id.clone());
                self.emit(
                    events,
                    plan,
                    "engine-preserved",
                    EventLevel::Warning,
                    Some("remove-engines"),
                    &format!(
                        "{} a été conservé : le reçu ne contient pas de gestionnaire exploitable",
                        component.id
                    ),
                    None,
                    None,
                    None,
                    json!({ "component": component.id }),
                );
                continue;
            }
            for removal in removals {
                let identity = format!("{}:{}", removal.manager, removal.package);
                if !removed_packages.insert(identity) {
                    continue;
                }
                let args = removal.args.iter().map(OsStr::new).collect::<Vec<_>>();
                match run_checked(removal.program.as_str(), &args).await {
                    Ok(()) => self.emit(
                        events,
                        plan,
                        "engine-removed",
                        EventLevel::Success,
                        Some("remove-engines"),
                        &format!("{} retiré via {}", removal.package, removal.manager),
                        None,
                        None,
                        None,
                        json!({
                            "component": component.id,
                            "manager": removal.manager,
                            "package": removal.package
                        }),
                    ),
                    Err(message) => self.emit(
                        events,
                        plan,
                        "engine-preserved",
                        EventLevel::Warning,
                        Some("remove-engines"),
                        &format!("{} a été conservé : {message}", component.id),
                        None,
                        None,
                        None,
                        {
                            self.state
                                .lock()
                                .map_err(|error| error.to_string())?
                                .packages_preserved
                                .push(component.id.clone());
                            json!({
                                "component": component.id,
                                "manager": removal.manager,
                                "package": removal.package
                            })
                        },
                    ),
                }
            }
        }

        if plan.request.remove_preexisting_engines {
            let selected = plan
                .request
                .selected_engines
                .iter()
                .map(String::as_str)
                .collect::<HashSet<_>>();
            for engine in self.initial.engines.iter().filter(|engine| {
                engine.installed
                    && !engine.installed_by_fileflow
                    && selected.contains(engine.id.as_str())
            }) {
                let Some((manager, package)) = legacy_package(plan.platform, &engine.id) else {
                    self.state
                        .lock()
                        .map_err(|error| error.to_string())?
                        .packages_preserved
                        .push(format!("preexisting:{}", engine.id));
                    self.emit(
                        events,
                        plan,
                        "engine-preserved",
                        EventLevel::Warning,
                        Some("remove-engines"),
                        &format!(
                            "{} conservé : aucun paquet système exact n’est identifiable pour ce moteur préexistant",
                            engine.label
                        ),
                        None,
                        None,
                        None,
                        json!({ "component": engine.id, "preexisting": true }),
                    );
                    continue;
                };
                let Some(removal) = package_removal_command(plan.platform, manager, package) else {
                    self.state
                        .lock()
                        .map_err(|error| error.to_string())?
                        .packages_preserved
                        .push(format!("preexisting:{}", engine.id));
                    self.emit(
                        events,
                        plan,
                        "engine-preserved",
                        EventLevel::Warning,
                        Some("remove-engines"),
                        &format!(
                            "{} conservé : le gestionnaire {manager} n’est pas disponible",
                            engine.label
                        ),
                        None,
                        None,
                        None,
                        json!({
                            "component": engine.id,
                            "preexisting": true,
                            "manager": manager,
                            "package": package
                        }),
                    );
                    continue;
                };
                let identity = format!("{}:{}", removal.manager, removal.package);
                if !removed_packages.insert(identity) {
                    continue;
                }
                self.emit(
                    events,
                    plan,
                    "engine-expert-removal",
                    EventLevel::Warning,
                    Some("remove-engines"),
                    &format!(
                        "Mode expert : retrait de {} via {}",
                        engine.label, removal.manager
                    ),
                    None,
                    None,
                    None,
                    json!({
                        "component": engine.id,
                        "preexisting": true,
                        "manager": removal.manager,
                        "package": removal.package
                    }),
                );
                let args = removal.args.iter().map(OsStr::new).collect::<Vec<_>>();
                if let Err(message) = run_checked(removal.program.as_str(), &args).await {
                    self.state
                        .lock()
                        .map_err(|error| error.to_string())?
                        .packages_preserved
                        .push(format!("preexisting:{}", engine.id));
                    self.emit(
                        events,
                        plan,
                        "engine-preserved",
                        EventLevel::Warning,
                        Some("remove-engines"),
                        &format!("{} a été conservé : {message}", engine.label),
                        None,
                        None,
                        None,
                        json!({ "component": engine.id, "preexisting": true }),
                    );
                } else {
                    self.emit(
                        events,
                        plan,
                        "engine-removed",
                        EventLevel::Success,
                        Some("remove-engines"),
                        &format!("{} retiré en mode expert", engine.label),
                        None,
                        None,
                        None,
                        json!({ "component": engine.id, "preexisting": true }),
                    );
                }
            }
        }
        Ok(())
    }

    async fn remove_selected_data(&self, plan: &SetupPlan) -> Result<(), String> {
        let paths = removable_data_paths(plan.platform);
        let quarantine_root = self.operation_dir.join("data-quarantine");
        let mut quarantined = Vec::new();
        let mut database_backups = Vec::new();
        let removal = async {
            if plan.request.remove_cache {
                move_paths_to_quarantine(&paths.cache, &quarantine_root, "cache", &mut quarantined)
                    .await?;
            }
            if plan.request.remove_history {
                move_paths_to_quarantine(
                    &paths.history,
                    &quarantine_root,
                    "history",
                    &mut quarantined,
                )
                .await?;
            }
            if plan.request.remove_settings {
                move_paths_to_quarantine(
                    &paths.settings,
                    &quarantine_root,
                    "settings",
                    &mut quarantined,
                )
                .await?;
            }
            if (plan.request.remove_settings || plan.request.remove_history)
                && let Some(database) = fileflow_database_candidates(plan.platform)
                    .into_iter()
                    .find(|candidate| candidate.is_file())
            {
                let backup = self.operation_dir.join("fileflow.sqlite3.backup");
                fs::copy(&database, &backup)
                    .await
                    .map_err(|error| error.to_string())?;
                database_backups.push((database.clone(), backup));
                clean_fileflow_database(
                    &database,
                    plan.request.remove_settings,
                    plan.request.remove_history,
                )?;
            }
            Ok::<(), String>(())
        }
        .await;
        if let Err(error) = removal {
            restore_quarantined_data(&quarantined, &database_backups).await;
            return Err(error);
        }
        let mut state = self.state.lock().map_err(|error| error.to_string())?;
        state.data_quarantine.extend(quarantined);
        state.database_backups.extend(database_backups);
        Ok(())
    }

    async fn finalize_uninstall(
        &self,
        plan: &SetupPlan,
        events: &dyn EventSink,
    ) -> Result<(), String> {
        let mut leftovers = Vec::new();
        let quarantine = self
            .state
            .lock()
            .map_err(|error| error.to_string())?
            .uninstall_quarantine
            .clone();
        if let Some(quarantine) = quarantine
            && let Err(error) = remove_path_if_exists(&quarantine).await
        {
            leftovers.push(format!("{}: {error}", quarantine.display()));
        }
        let (data_quarantine, database_backups, packages_preserved) = {
            let state = self.state.lock().map_err(|error| error.to_string())?;
            (
                state.data_quarantine.clone(),
                state.database_backups.clone(),
                state.packages_preserved.clone(),
            )
        };
        for (_, quarantine) in data_quarantine {
            if let Err(error) = remove_path_if_exists(&quarantine).await {
                leftovers.push(format!("{}: {error}", quarantine.display()));
            }
        }
        for (_, backup) in database_backups {
            if let Err(error) = remove_path_if_exists(&backup).await {
                leftovers.push(format!("{}: {error}", backup.display()));
            }
        }
        if packages_preserved.is_empty()
            && self.initial.receipt_path.exists()
            && let Err(error) = fs::remove_file(&self.initial.receipt_path).await
        {
            leftovers.push(format!("{}: {error}", self.initial.receipt_path.display()));
        }
        if self.initial.platform == Platform::Linux {
            let home = home_dir();
            let local_share = home.join(".local").join("share");
            let applications = local_share.join("applications");
            let hicolor = local_share.join("icons").join("hicolor");
            let mut paths = vec![
                home.join(".local/bin/fileflow"),
                applications.join("fileflow.desktop"),
            ];
            paths.extend(
                [32, 64, 128, 256, 512]
                    .map(|size| hicolor.join(format!("{size}x{size}/apps/fileflow.png"))),
            );
            for path in paths {
                if let Err(error) = remove_path_if_exists(&path).await {
                    leftovers.push(format!("{}: {error}", path.display()));
                }
            }
            if command_exists("update-desktop-database") {
                let _ = run_checked("update-desktop-database", &[applications.as_os_str()]).await;
            }
            if command_exists("gtk-update-icon-cache") {
                let _ = run_checked(
                    "gtk-update-icon-cache",
                    &[OsStr::new("-f"), OsStr::new("-t"), hicolor.as_os_str()],
                )
                .await;
            }
        } else if self.initial.platform == Platform::Windows {
            for path in windows_shortcut_candidates() {
                // Never fail the uninstall only because another installer has already
                // removed a Start Menu entry.
                if path.exists()
                    && let Err(error) = remove_path_if_exists(&path).await
                {
                    leftovers.push(format!("{}: {error}", path.display()));
                }
            }
        }
        if packages_preserved.is_empty()
            && let Some(path) = self
                .initial
                .receipt
                .as_ref()
                .and_then(|receipt| {
                    receipt
                        .components
                        .iter()
                        .find(|item| item.id == "fileflow-setup")
                })
                .and_then(|item| item.path.clone())
            && let Err(error) = schedule_maintenance_removal(&path).await
        {
            leftovers.push(format!("{}: {error}", path.display()));
        }
        if !packages_preserved.is_empty() {
            leftovers.push(format!(
                "moteurs/bibliothèques conservés : {} ; reçu et outil de maintenance gardés pour réessayer",
                packages_preserved.join(", ")
            ));
        }
        if !leftovers.is_empty() {
            self.emit(
                events,
                plan,
                "uninstall-leftovers",
                EventLevel::Warning,
                Some("uninstall-report"),
                "FileFlow est retiré ; quelques restes non critiques sont listés dans le journal",
                None,
                None,
                None,
                json!({ "leftovers": leftovers }),
            );
        }
        Ok(())
    }

    async fn run_streaming(
        &self,
        plan: &SetupPlan,
        step_id: &str,
        command: &mut Command,
        events: &dyn EventSink,
        cancellation: &AtomicBool,
    ) -> Result<(), String> {
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        hide_process(command);
        isolate_process(command);
        let mut child = command.spawn().map_err(|error| error.to_string())?;
        let stdout = child
            .stdout
            .take()
            .map(BufReader::new)
            .map(|reader| reader.lines());
        let stderr = child
            .stderr
            .take()
            .map(BufReader::new)
            .map(|reader| reader.lines());
        let mut stdout = stdout.ok_or_else(|| "stdout indisponible".to_string())?;
        let mut stderr = stderr.ok_or_else(|| "stderr indisponible".to_string())?;
        let mut stdout_done = false;
        let mut stderr_done = false;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(45 * 60);
        let mut heartbeat = tokio::time::interval(Duration::from_millis(250));
        while !stdout_done || !stderr_done {
            tokio::select! {
                _ = heartbeat.tick() => {
                    if cancellation.load(Ordering::Relaxed) {
                        terminate_process_tree(&mut child).await;
                        return Err("opération annulée".into());
                    }
                    if tokio::time::Instant::now() >= deadline {
                        terminate_process_tree(&mut child).await;
                        return Err("le processus d’installation a dépassé 45 minutes".into());
                    }
                }
                line = stdout.next_line(), if !stdout_done => match line {
                    Ok(Some(line)) => self.emit_log(events, plan, step_id, &line, false),
                    Ok(None) => stdout_done = true,
                    Err(error) => return Err(error.to_string()),
                },
                line = stderr.next_line(), if !stderr_done => match line {
                    Ok(Some(line)) => self.emit_log(events, plan, step_id, &line, true),
                    Ok(None) => stderr_done = true,
                    Err(error) => return Err(error.to_string()),
                },
            }
        }
        let status = match tokio::time::timeout(Duration::from_secs(30), child.wait()).await {
            Ok(result) => result.map_err(|error| error.to_string())?,
            Err(_) => {
                terminate_process_tree(&mut child).await;
                return Err("le processus ne s’est pas fermé après avoir terminé sa sortie".into());
            }
        };
        if !status.success() {
            return Err(format!("le processus a terminé avec {status}"));
        }
        Ok(())
    }

    fn emit_log(
        &self,
        events: &dyn EventSink,
        plan: &SetupPlan,
        step_id: &str,
        line: &str,
        warning: bool,
    ) {
        let clean = redact_line(line);
        let parsed = runtime_log_status(&clean);
        self.emit(
            events,
            plan,
            if parsed.is_some() {
                "resource-progress"
            } else {
                "process-log"
            },
            if warning {
                EventLevel::Warning
            } else if matches!(parsed, Some(("ready", _))) {
                EventLevel::Success
            } else {
                EventLevel::Info
            },
            Some(step_id),
            &clean,
            None,
            None,
            None,
            parsed
                .map(|(status, resource)| json!({ "status": status, "resource": resource }))
                .unwrap_or_else(|| json!({})),
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn emit(
        &self,
        events: &dyn EventSink,
        plan: &SetupPlan,
        event_type: &str,
        level: EventLevel,
        step_id: Option<&str>,
        message: &str,
        completed: Option<u64>,
        total: Option<u64>,
        unit: Option<&str>,
        detail: serde_json::Value,
    ) {
        events.emit(SetupEvent {
            operation_id: plan.operation_id,
            sequence: self.sequence.fetch_add(1, Ordering::Relaxed),
            timestamp: Utc::now(),
            event_type: event_type.into(),
            level,
            step_id: step_id.map(str::to_owned),
            message: message.into(),
            completed,
            total,
            unit: unit.map(str::to_owned),
            detail,
        });
    }
}

#[async_trait]
impl SetupActionAdapter for SystemSetupAdapter {
    async fn apply(
        &self,
        plan: &SetupPlan,
        step: &PlanStep,
        events: &dyn EventSink,
        cancellation: &AtomicBool,
    ) -> Result<ActionOutcome, String> {
        if plan.request.dry_run || step.operation == PlannedOperation::Preserve {
            return Ok(ActionOutcome {
                message: Some(if plan.request.dry_run {
                    "simulation : aucune modification".into()
                } else {
                    "élément conservé".into()
                }),
                receipt: None,
            });
        }
        match step.id.as_str() {
            "preflight" => {
                if self.initial.application.running && plan.request.mode != SetupMode::Uninstall {
                    self.emit(
                        events,
                        plan,
                        "application-closing",
                        EventLevel::Info,
                        Some("preflight"),
                        "FileFlow est ouvert ; fermeture propre avant de continuer",
                        None,
                        None,
                        None,
                        json!({}),
                    );
                    self.stop_application().await?;
                    if probe_system()
                        .map_err(|error| error.to_string())?
                        .application
                        .running
                    {
                        return Err("FileFlow est toujours ouvert après la tentative de fermeture automatique".into());
                    }
                }
            }
            "release" => self.fetch_release(plan, events, cancellation).await?,
            "application" => self.install_application(plan, events).await?,
            "integration" => self.ensure_system_integration(plan, events).await?,
            "engines" => {
                self.install_engines(plan, step, events, cancellation)
                    .await?
            }
            "engine-postcheck" => self.verify_selected_engines(plan, step, events)?,
            "maintenance" => self.persist_maintenance().await?,
            "postcheck" | "doctor" | "uninstall-postcheck" => {
                self.postcheck(plan, events, cancellation).await?
            }
            "receipt" => self.write_install_receipt(plan).await?,
            "launch" => self.launch_application().await?,
            "stop" => self.stop_application().await?,
            "remove-application" => self.quarantine_application().await?,
            "remove-engines" => self.remove_owned_engines(plan, events).await?,
            "remove-data" => self.remove_selected_data(plan).await?,
            "uninstall-report" => self.finalize_uninstall(plan, events).await?,
            _ => {}
        }
        Ok(ActionOutcome::default())
    }

    async fn rollback(
        &self,
        _plan: &SetupPlan,
        step: &PlanStep,
        _events: &dyn EventSink,
    ) -> Result<(), String> {
        match step.id.as_str() {
            "application" => {
                let (installed, backup) = {
                    let state = self.state.lock().map_err(|error| error.to_string())?;
                    (
                        state.installed_application.clone(),
                        state.application_backup.clone(),
                    )
                };
                if let Some(installed) = installed {
                    let destination = if self.initial.platform == Platform::Windows {
                        self.initial
                            .application
                            .path
                            .as_deref()
                            .and_then(Path::parent)
                            .or_else(|| installed.parent())
                            .unwrap_or(&installed)
                            .to_owned()
                    } else {
                        installed
                    };
                    if destination.exists() {
                        remove_path_if_exists(&destination).await?;
                    }
                    if let Some(backup) = backup
                        && backup.exists()
                    {
                        fs::rename(backup, destination)
                            .await
                            .map_err(|error| error.to_string())?;
                    }
                }
            }
            "engines" => {
                let installed = self
                    .state
                    .lock()
                    .map_err(|error| error.to_string())?
                    .packages_installed
                    .clone();
                for package in installed {
                    if let Some(removal) = package_removal_command(
                        self.initial.platform,
                        &package.manager,
                        &package.package,
                    ) {
                        let args = removal.args.iter().map(OsStr::new).collect::<Vec<_>>();
                        run_checked(removal.program.as_str(), &args).await?;
                    }
                }
            }
            "maintenance" => {
                let (destination, backup, changed) = {
                    let state = self.state.lock().map_err(|error| error.to_string())?;
                    (
                        state.maintenance_path.clone(),
                        state.maintenance_backup.clone(),
                        state.maintenance_changed,
                    )
                };
                if changed && let Some(destination) = destination {
                    if destination.exists() {
                        remove_path_if_exists(&destination).await?;
                    }
                    if let Some(backup) = backup
                        && backup.exists()
                    {
                        fs::rename(backup, destination)
                            .await
                            .map_err(|error| error.to_string())?;
                    }
                }
            }
            "remove-data" => {
                let (quarantined, database_backups) = {
                    let state = self.state.lock().map_err(|error| error.to_string())?;
                    (
                        state.data_quarantine.clone(),
                        state.database_backups.clone(),
                    )
                };
                for (original, quarantine) in quarantined.into_iter().rev() {
                    if quarantine.exists() && !original.exists() {
                        if let Some(parent) = original.parent() {
                            fs::create_dir_all(parent)
                                .await
                                .map_err(|error| error.to_string())?;
                        }
                        fs::rename(quarantine, original)
                            .await
                            .map_err(|error| error.to_string())?;
                    }
                }
                for (original, backup) in database_backups.into_iter().rev() {
                    if backup.exists() {
                        fs::copy(backup, original)
                            .await
                            .map_err(|error| error.to_string())?;
                    }
                }
            }
            "remove-application" => {
                let quarantine = self
                    .state
                    .lock()
                    .map_err(|error| error.to_string())?
                    .uninstall_quarantine
                    .clone();
                if let (Some(original), Some(quarantine)) =
                    (&self.initial.application.path, quarantine)
                {
                    let destination = if self.initial.platform == Platform::Windows {
                        original.parent().unwrap_or(original)
                    } else {
                        original.as_path()
                    };
                    if quarantine.exists() && !destination.exists() {
                        fs::rename(&quarantine, destination)
                            .await
                            .map_err(|error| error.to_string())?;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}

struct DataPaths {
    cache: Vec<PathBuf>,
    settings: Vec<PathBuf>,
    history: Vec<PathBuf>,
}

fn removable_data_paths(platform: Platform) -> DataPaths {
    match platform {
        Platform::Macos => {
            let home = home_dir();
            let support = home
                .join("Library")
                .join("Application Support")
                .join("FileFlow");
            DataPaths {
                cache: vec![
                    home.join("Library")
                        .join("Caches")
                        .join("com.fileflow.desktop"),
                ],
                settings: vec![support.join("preferences.json")],
                history: vec![support.join("history.sqlite")],
            }
        }
        Platform::Linux => {
            let home = home_dir();
            DataPaths {
                cache: vec![home.join(".cache").join("fileflow")],
                settings: vec![home.join(".config").join("fileflow")],
                history: vec![
                    home.join(".local")
                        .join("share")
                        .join("fileflow")
                        .join("history.sqlite"),
                ],
            }
        }
        Platform::Windows => {
            let local = std::env::var_os("LOCALAPPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|| home_dir().join("AppData").join("Local"));
            let roaming = std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|| home_dir().join("AppData").join("Roaming"));
            DataPaths {
                cache: vec![local.join("FileFlow").join("cache")],
                settings: vec![roaming.join("FileFlow").join("preferences.json")],
                history: vec![local.join("FileFlow").join("history.sqlite")],
            }
        }
    }
}

fn fileflow_database_candidates(platform: Platform) -> Vec<PathBuf> {
    let home = home_dir();
    match platform {
        Platform::Macos => vec![
            home.join("Library/Application Support/com.fileflow.desktop/fileflow.sqlite3"),
            home.join("Library/Application Support/FileFlow/fileflow.sqlite3"),
        ],
        Platform::Linux => vec![
            std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".local/share"))
                .join("com.fileflow.desktop/fileflow.sqlite3"),
            home.join(".local/share/fileflow/fileflow.sqlite3"),
        ],
        Platform::Windows => vec![
            std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join("AppData/Roaming"))
                .join("com.fileflow.desktop/fileflow.sqlite3"),
            std::env::var_os("LOCALAPPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join("AppData/Local"))
                .join("FileFlow/fileflow.sqlite3"),
        ],
    }
}

fn clean_fileflow_database(
    path: &Path,
    remove_settings: bool,
    remove_history: bool,
) -> Result<(), String> {
    let connection = rusqlite::Connection::open(path).map_err(|error| error.to_string())?;
    if remove_settings {
        connection
            .execute("DELETE FROM settings", [])
            .map_err(|error| error.to_string())?;
    }
    if remove_history {
        connection
            .execute_batch(
                "DELETE FROM history; DELETE FROM account_history; DELETE FROM automation_jobs;",
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

async fn schedule_maintenance_removal(path: &Path) -> Result<(), String> {
    if cfg!(target_os = "windows") {
        let mut command = Command::new(windows_powershell_program());
        command
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-WindowStyle",
                "Hidden",
                "-Command",
                "Start-Sleep -Seconds 2; Remove-Item -LiteralPath $env:FILEFLOW_PS_REMOVE -Force -Recurse",
            ])
            .env("FILEFLOW_PS_REMOVE", path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        hide_process(&mut command);
        command.spawn().map_err(|error| error.to_string())?;
        Ok(())
    } else {
        remove_path_if_exists(path).await
    }
}

fn windows_shortcut_candidates() -> Vec<PathBuf> {
    let roaming = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join("AppData").join("Roaming"));
    let common = std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
    vec![
        roaming.join(r"Microsoft\Windows\Start Menu\Programs\FileFlow.lnk"),
        roaming.join(r"Microsoft\Windows\Start Menu\Programs\FileFlow\FileFlow.lnk"),
        common.join(r"Microsoft\Windows\Start Menu\Programs\FileFlow.lnk"),
        common.join(r"Microsoft\Windows\Start Menu\Programs\FileFlow\FileFlow.lnk"),
    ]
}

async fn install_windows_integration(application: &Path) -> Result<(), String> {
    let roaming = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join("AppData").join("Roaming"));
    let programs = roaming.join(r"Microsoft\Windows\Start Menu\Programs");
    fs::create_dir_all(&programs)
        .await
        .map_err(|error| error.to_string())?;
    let shortcut = programs.join("FileFlow.lnk");
    let working_directory = application.parent().unwrap_or_else(|| Path::new(r"C:\"));
    let script = r#"$ErrorActionPreference='Stop'; $Target=$env:FILEFLOW_PS_TARGET; $Shortcut=$env:FILEFLOW_PS_SHORTCUT; $WorkingDirectory=$env:FILEFLOW_PS_WORKING_DIRECTORY; $shell=New-Object -ComObject WScript.Shell; $link=$shell.CreateShortcut($Shortcut); $link.TargetPath=$Target; $link.WorkingDirectory=$WorkingDirectory; $link.IconLocation=($Target + ',0'); $link.Description='FileFlow — conversion et organisation locale de fichiers'; $link.Save(); if (-not (Test-Path -LiteralPath $Shortcut)) { throw 'FileFlow shortcut was not created' }"#;
    run_windows_powershell(
        script,
        &[
            ("FILEFLOW_PS_TARGET", application.as_os_str()),
            ("FILEFLOW_PS_SHORTCUT", shortcut.as_os_str()),
            (
                "FILEFLOW_PS_WORKING_DIRECTORY",
                working_directory.as_os_str(),
            ),
        ],
    )
    .await?;
    if !shortcut.is_file() {
        return Err("le raccourci FileFlow du menu Démarrer n’a pas été créé".into());
    }
    Ok(())
}

async fn install_linux_integration(app: &Path, resource_dir: &Path) -> Result<(), String> {
    let home = home_dir();
    let local_share = home.join(".local").join("share");
    let bin = home.join(".local").join("bin");
    let applications = local_share.join("applications");
    fs::create_dir_all(&bin)
        .await
        .map_err(|error| error.to_string())?;
    fs::create_dir_all(&applications)
        .await
        .map_err(|error| error.to_string())?;

    let wrapper = bin.join("fileflow");
    fs::write(
        &wrapper,
        format!(
            "#!/usr/bin/env bash\nset -e\nexport APPIMAGE_EXTRACT_AND_RUN=1\nexec \"{}\" \"$@\"\n",
            app.display()
        ),
    )
    .await
    .map_err(|error| error.to_string())?;
    run_checked("chmod", &[OsStr::new("+x"), wrapper.as_os_str()]).await?;

    let icon_sources = [
        (32, "32x32.png"),
        (64, "64x64.png"),
        (128, "128x128.png"),
        (256, "128x128@2x.png"),
        (512, "icon.png"),
    ];
    for (size, resource_name) in icon_sources {
        let icon_source = find_resource(resource_dir, resource_name)
            .or_else(|| find_resource(resource_dir, "icon.png"))
            .ok_or_else(|| "icône FileFlow absente des ressources du Setup".to_string())?;
        let icons = local_share
            .join("icons")
            .join("hicolor")
            .join(format!("{size}x{size}"))
            .join("apps");
        fs::create_dir_all(&icons)
            .await
            .map_err(|error| error.to_string())?;
        fs::copy(&icon_source, icons.join("fileflow.png"))
            .await
            .map_err(|error| format!("copie de l’icône FileFlow impossible: {error}"))?;
    }

    let desktop_path = applications.join("fileflow.desktop");
    fs::write(
        &desktop_path,
        format!(
            "[Desktop Entry]\nVersion=1.0\nType=Application\nName=FileFlow\nGenericName=File converter\nComment=Conversion, compression et organisation locale de fichiers\nExec=\"{}\" %U\nTryExec={}\nIcon=fileflow\nTerminal=false\nCategories=Utility;Office;FileTools;\nStartupNotify=true\nStartupWMClass=FileFlow\nKeywords=PDF;Image;Video;Archive;OCR;Conversion;\nMimeType=application/pdf;image/png;image/jpeg;image/webp;video/mp4;audio/mpeg;application/zip;\n",
            wrapper.display(),
            wrapper.display()
        ),
    )
    .await
    .map_err(|error| error.to_string())?;
    run_checked("chmod", &[OsStr::new("+x"), desktop_path.as_os_str()]).await?;

    // Les caches sont facultatifs dans freedesktop, mais les rafraîchir rend le
    // lanceur et le logo visibles immédiatement sur GNOME/KDE lorsqu'ils existent.
    if command_exists("update-desktop-database") {
        let _ = run_checked("update-desktop-database", &[applications.as_os_str()]).await;
    }
    if command_exists("gtk-update-icon-cache") {
        let hicolor = local_share.join("icons").join("hicolor");
        let _ = run_checked(
            "gtk-update-icon-cache",
            &[OsStr::new("-f"), OsStr::new("-t"), hicolor.as_os_str()],
        )
        .await;
    }

    if !desktop_path.is_file()
        || !wrapper.is_file()
        || !local_share
            .join("icons/hicolor/512x512/apps/fileflow.png")
            .is_file()
    {
        return Err("le lanceur ou l’icône FileFlow n’a pas été installé correctement".into());
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PackageRemoval {
    manager: String,
    package: String,
    program: String,
    args: Vec<String>,
}

fn read_package_report(path: &Path) -> Result<Vec<InstalledPackage>, String> {
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let source = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    if source
        .trim_matches(|character: char| character == '\u{feff}' || character.is_whitespace())
        .is_empty()
    {
        return Ok(Vec::new());
    }
    let mut records = Vec::new();
    let mut seen = HashSet::new();
    for (index, line) in source.lines().enumerate() {
        let fields = line.split('\t').collect::<Vec<_>>();
        let component = fields
            .first()
            .copied()
            .unwrap_or_default()
            .trim_start_matches('\u{feff}');
        if fields.len() != 4
            || !safe_package_token(component)
            || !safe_package_token(fields[1])
            || !safe_package_token(fields[2])
            || !matches!(fields[3], "engine" | "integration")
        {
            return Err(format!(
                "rapport de paquets invalide à la ligne {}",
                index + 1
            ));
        }
        let identity = format!("{}:{}:{}", component, fields[1], fields[2]);
        if seen.insert(identity) {
            records.push(InstalledPackage {
                component_id: component.into(),
                manager: fields[1].into(),
                package: fields[2].into(),
                integration: fields[3] == "integration",
            });
        }
    }
    Ok(records)
}

fn safe_package_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 160
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || matches!(character, '.' | '+' | '_' | '@' | '-' | '/' | ':')
        })
}

fn removal_commands(platform: Platform, component: &ReceiptComponent) -> Vec<PackageRemoval> {
    if let Some(manager) = component.package_manager.as_deref() {
        let commands = component
            .packages
            .iter()
            .filter_map(|package| package_removal_command(platform, manager, package))
            .collect::<Vec<_>>();
        if !commands.is_empty() {
            return commands;
        }
    }
    legacy_package(platform, &component.id)
        .and_then(|(manager, package)| package_removal_command(platform, manager, package))
        .into_iter()
        .collect()
}

fn package_removal_command(
    platform: Platform,
    manager: &str,
    package: &str,
) -> Option<PackageRemoval> {
    if !safe_package_token(manager) || !safe_package_token(package) {
        return None;
    }
    let (mut program, mut args, elevated) = base_removal_command(manager, package)?;
    if !command_exists(&program) {
        return None;
    }
    if elevated && platform == Platform::Linux && !is_root() {
        if command_exists("pkexec") {
            args.insert(0, program);
            program = "pkexec".into();
        } else if command_exists("sudo") {
            args.insert(0, program);
            program = "sudo".into();
        } else {
            return None;
        }
    }
    Some(PackageRemoval {
        manager: manager.into(),
        package: package.into(),
        program,
        args,
    })
}

fn base_removal_command(manager: &str, package: &str) -> Option<(String, Vec<String>, bool)> {
    let (program, args, elevated) = match manager {
        "apt" => ("apt-get", vec!["purge", "-y", package], true),
        "dnf" => ("dnf", vec!["remove", "-y", package], true),
        "zypper" => (
            "zypper",
            vec!["--non-interactive", "remove", "-y", package],
            true,
        ),
        "pacman" => ("pacman", vec!["-Rns", "--noconfirm", package], true),
        "brew" => ("brew", vec!["uninstall", package], false),
        "brew-cask" => ("brew", vec!["uninstall", "--cask", package], false),
        "pipx" => ("pipx", vec!["uninstall", package], false),
        "flatpak" => ("flatpak", vec!["uninstall", "-y", package], false),
        "winget" => (
            "winget.exe",
            vec![
                "uninstall",
                "--id",
                package,
                "--exact",
                "--silent",
                "--disable-interactivity",
            ],
            false,
        ),
        "choco" => (
            "choco.exe",
            vec!["uninstall", package, "-y", "--no-progress"],
            false,
        ),
        "scoop" => ("scoop", vec!["uninstall", package], false),
        _ => return None,
    };
    Some((
        program.into(),
        args.into_iter().map(str::to_owned).collect(),
        elevated,
    ))
}

fn legacy_package(platform: Platform, engine: &str) -> Option<(&'static str, &'static str)> {
    match platform {
        Platform::Macos if command_exists("brew") => match engine {
            "ffmpeg" => Some(("brew", "ffmpeg")),
            "vips" => Some(("brew", "vips")),
            "imagemagick" => Some(("brew", "imagemagick")),
            "qpdf" => Some(("brew", "qpdf")),
            "img2pdf" => Some(("brew", "img2pdf")),
            "poppler" => Some(("brew", "poppler")),
            "ghostscript" => Some(("brew", "ghostscript")),
            "tesseract" => Some(("brew", "tesseract")),
            "ocrmypdf" => Some(("brew", "ocrmypdf")),
            "libreoffice" => Some(("brew-cask", "libreoffice")),
            "pandoc" => Some(("brew", "pandoc")),
            "browser" => Some(("brew-cask", "google-chrome")),
            "exiftool" => Some(("brew", "exiftool")),
            "sevenzip" => Some(("brew", "sevenzip")),
            "zstd" => Some(("brew", "zstd")),
            "lz4" => Some(("brew", "lz4")),
            _ => None,
        },
        Platform::Linux if command_exists("apt-get") => match engine {
            "ffmpeg" => Some(("apt", "ffmpeg")),
            "vips" => Some(("apt", "libvips-tools")),
            "imagemagick" => Some(("apt", "imagemagick")),
            "qpdf" => Some(("apt", "qpdf")),
            "img2pdf" => Some(("apt", "img2pdf")),
            "poppler" => Some(("apt", "poppler-utils")),
            "ghostscript" => Some(("apt", "ghostscript")),
            "tesseract" => Some(("apt", "tesseract-ocr")),
            "ocrmypdf" => Some(("apt", "ocrmypdf")),
            "libreoffice" => Some(("apt", "libreoffice")),
            "pandoc" => Some(("apt", "pandoc")),
            "browser" => Some(("apt", "chromium")),
            "exiftool" => Some(("apt", "libimage-exiftool-perl")),
            "sevenzip" => Some(("apt", "7zip")),
            "zstd" => Some(("apt", "zstd")),
            "lz4" => Some(("apt", "lz4")),
            _ => None,
        },
        Platform::Windows if command_exists("winget.exe") => match engine {
            "ffmpeg" => Some(("winget", "Gyan.FFmpeg")),
            "vips" => Some(("winget", "libvips.libvips")),
            "imagemagick" => Some(("winget", "ImageMagick.ImageMagick")),
            "qpdf" => Some(("winget", "QPDF.QPDF")),
            "poppler" => Some(("winget", "oschwartz10612.Poppler")),
            "ghostscript" => Some(("winget", "ArtifexSoftware.GhostScript")),
            "tesseract" => Some(("winget", "tesseract-ocr.tesseract")),
            "libreoffice" => Some(("winget", "TheDocumentFoundation.LibreOffice")),
            "pandoc" => Some(("winget", "JohnMacFarlane.Pandoc")),
            "browser" => Some(("winget", "Microsoft.Edge")),
            "exiftool" => Some(("winget", "OliverBetz.ExifTool")),
            "sevenzip" => Some(("winget", "7zip.7zip")),
            "zstd" => Some(("winget", "Facebook.Zstandard")),
            "lz4" => Some(("winget", "LZ4.LZ4")),
            _ => None,
        },
        _ => None,
    }
}

async fn run_checked<P: AsRef<OsStr>>(program: P, arguments: &[&OsStr]) -> Result<(), String> {
    let mut command = Command::new(program);
    command
        .args(arguments)
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    hide_process(&mut command);
    let output = command.output().await.map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

fn windows_powershell_program() -> &'static str {
    if command_exists("pwsh.exe") {
        "pwsh.exe"
    } else {
        "powershell.exe"
    }
}

async fn run_windows_powershell(
    script: &str,
    environment: &[(&str, &OsStr)],
) -> Result<String, String> {
    let mut command = Command::new(windows_powershell_program());
    command.args([
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        script,
    ]);
    for &(name, value) in environment {
        command.env(name, value);
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    hide_process(&mut command);
    let output = command.output().await.map_err(|error| error.to_string())?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        return Err(if stderr.is_empty() { stdout } else { stderr });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn doctor_command(script: &Path) -> Command {
    let mut command = if cfg!(target_os = "windows") {
        let mut value = Command::new(windows_powershell_program());
        value.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]);
        value
    } else {
        Command::new("bash")
    };
    command
        .arg(script)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    hide_process(&mut command);
    command
}

fn application_executable(platform: Platform, application: &Path) -> Result<PathBuf, String> {
    let executable = match platform {
        Platform::Macos => application
            .join("Contents")
            .join("MacOS")
            .join("fileflow-desktop"),
        Platform::Windows | Platform::Linux => application.to_owned(),
    };
    executable
        .is_file()
        .then_some(executable)
        .ok_or_else(|| "exécutable FileFlow absent après installation".into())
}

#[cfg(unix)]
fn isolate_process(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.as_std_mut().process_group(0);
}

#[cfg(not(unix))]
fn isolate_process(_command: &mut Command) {}

async fn terminate_process_tree(child: &mut tokio::process::Child) {
    let Some(pid) = child.id() else {
        return;
    };
    if cfg!(target_os = "windows") {
        let mut command = Command::new("taskkill.exe");
        command.args(["/PID", &pid.to_string(), "/T", "/F"]);
        hide_process(&mut command);
        let _ = command.status().await;
    } else {
        let process_group = format!("-{pid}");

        let mut term = Command::new("kill");
        term.arg("-TERM");
        if !cfg!(target_os = "macos") {
            term.arg("--");
        }
        term.arg(&process_group);
        let _ = term.status().await;

        tokio::time::sleep(Duration::from_millis(350)).await;

        if child.try_wait().ok().flatten().is_none() {
            let mut kill = Command::new("kill");
            kill.arg("-KILL");
            if !cfg!(target_os = "macos") {
                kill.arg("--");
            }
            kill.arg(&process_group);
            let _ = kill.status().await;
        }
    }
    if tokio::time::timeout(Duration::from_secs(2), child.wait())
        .await
        .is_err()
    {
        let _ = child.kill().await;
    }
}

#[cfg(windows)]
async fn verify_windows_signature(path: &Path) -> Result<bool, String> {
    let status = run_windows_powershell(
        "$ErrorActionPreference='Stop'; Import-Module Microsoft.PowerShell.Security -ErrorAction Stop; (Get-AuthenticodeSignature -LiteralPath $env:FILEFLOW_PS_PATH).Status.ToString()",
        &[("FILEFLOW_PS_PATH", path.as_os_str())],
    )
    .await
    .map_err(|error| format!("contrôle Authenticode impossible: {error}"))?;
    match status.as_str() {
        "Valid" => Ok(true),
        "NotSigned" => Ok(false),
        _ => Err(format!("signature Authenticode refusée: {status}")),
    }
}

#[cfg(not(windows))]
async fn verify_windows_signature(_path: &Path) -> Result<bool, String> {
    Ok(true)
}

async fn replace_maintenance(
    source: &Path,
    destination: &Path,
    directory: bool,
    backup: &Path,
) -> Result<PathBuf, String> {
    let stage = destination.with_extension("installing");
    remove_path_if_exists(&stage).await?;
    remove_path_if_exists(backup).await?;
    if directory {
        run_checked("ditto", &[source.as_os_str(), stage.as_os_str()]).await?;
    } else {
        fs::copy(source, &stage)
            .await
            .map_err(|error| error.to_string())?;
    }
    if destination.exists() {
        fs::rename(destination, backup)
            .await
            .map_err(|error| format!("sauvegarde du centre de maintenance impossible: {error}"))?;
    }
    if let Err(error) = fs::rename(&stage, destination).await {
        if backup.exists() {
            let _ = fs::rename(backup, destination).await;
        }
        return Err(format!(
            "activation du centre de maintenance impossible: {error}"
        ));
    }
    Ok(backup.to_owned())
}

async fn move_paths_to_quarantine(
    paths: &[PathBuf],
    quarantine_root: &Path,
    category: &str,
    moved: &mut Vec<(PathBuf, PathBuf)>,
) -> Result<(), String> {
    for (index, original) in paths.iter().enumerate() {
        if !original.exists() {
            continue;
        }
        fs::create_dir_all(quarantine_root)
            .await
            .map_err(|error| error.to_string())?;
        let quarantine = quarantine_root.join(format!("{category}-{index}"));
        remove_path_if_exists(&quarantine).await?;
        fs::rename(original, &quarantine).await.map_err(|error| {
            format!(
                "mise en quarantaine de {} impossible: {error}",
                original.display()
            )
        })?;
        moved.push((original.clone(), quarantine));
    }
    Ok(())
}

async fn restore_quarantined_data(
    quarantined: &[(PathBuf, PathBuf)],
    database_backups: &[(PathBuf, PathBuf)],
) {
    for (original, backup) in database_backups.iter().rev() {
        if backup.exists() {
            let _ = fs::copy(backup, original).await;
        }
    }
    for (original, quarantine) in quarantined.iter().rev() {
        if quarantine.exists() && !original.exists() {
            if let Some(parent) = original.parent() {
                let _ = fs::create_dir_all(parent).await;
            }
            let _ = fs::rename(quarantine, original).await;
        }
    }
}

async fn remove_path_if_exists(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path).await {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path)
            .await
            .map_err(|error| error.to_string()),
        Ok(_) => fs::remove_file(path)
            .await
            .map_err(|error| error.to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn find_named_directory(root: &Path, name: &str, depth: usize) -> Option<PathBuf> {
    if depth == 0 {
        return None;
    }
    let entries = std::fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.file_name().and_then(OsStr::to_str) == Some(name) && path.is_dir() {
            return Some(path);
        }
        if path.is_dir()
            && let Some(found) = find_named_directory(&path, name, depth - 1)
        {
            return Some(found);
        }
    }
    None
}

fn find_resource(root: &Path, name: &str) -> Option<PathBuf> {
    if root.file_name().and_then(OsStr::to_str) == Some(name) && root.is_file() {
        return Some(root.to_owned());
    }
    let entries = std::fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.file_name().and_then(OsStr::to_str) == Some(name) && path.is_file() {
            return Some(path);
        }
        if path.is_dir()
            && let Some(found) = find_resource(&path, name)
        {
            return Some(found);
        }
    }
    None
}

fn command_exists(program: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|path| {
        path.join(
            if cfg!(target_os = "windows") && !program.ends_with(".exe") {
                format!("{program}.exe")
            } else {
                program.into()
            },
        )
        .is_file()
    })
}

fn is_writable(path: &Path) -> bool {
    let test = path.join(format!(".fileflow-write-test-{}", std::process::id()));
    match std::fs::write(&test, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(test);
            true
        }
        Err(_) => false,
    }
}

fn is_root() -> bool {
    cfg!(target_family = "unix")
        && std::process::Command::new("id")
            .arg("-u")
            .output()
            .is_ok_and(|output| output.stdout == b"0\n")
}

fn home_dir() -> PathBuf {
    std::env::var_os(if cfg!(target_os = "windows") {
        "USERPROFILE"
    } else {
        "HOME"
    })
    .map(PathBuf::from)
    .unwrap_or_else(|| PathBuf::from("."))
}

fn version_tuple(version: &str) -> (u64, u64, u64) {
    let mut parts = version
        .split(['-', '+'])
        .next()
        .unwrap_or_default()
        .split('.')
        .map(|part| part.parse::<u64>().unwrap_or(0));
    (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    )
}

fn doctor_script_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "doctor.ps1"
    } else {
        "doctor.sh"
    }
}

fn curl_program() -> &'static str {
    if cfg!(target_os = "windows") {
        "curl.exe"
    } else {
        "curl"
    }
}

fn runtime_log_status(line: &str) -> Option<(&'static str, &str)> {
    let trimmed = line.trim();
    for (prefix, status) in [
        ("[TRY]", "running"),
        ("[SETUP]", "running"),
        ("[AUTH]", "authorizing"),
        ("[OK]", "ready"),
        ("[MISS]", "missing"),
        ("[SKIP]", "skipped"),
        ("[WARN]", "warning"),
    ] {
        if let Some(resource) = trimmed.strip_prefix(prefix).map(str::trim)
            && !resource.is_empty()
        {
            return Some((status, resource));
        }
    }
    None
}

fn redact_line(line: &str) -> String {
    line.split_whitespace()
        .map(|word| {
            if word.starts_with("ghp_")
                || word.starts_with("github_pat_")
                || word.contains("TAURI_SIGNING_PRIVATE_KEY")
                || word.to_ascii_lowercase().contains("password=")
            {
                "[SECRET_MASQUÉ]"
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(windows)]
fn hide_process(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;
    command.as_std_mut().creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_process(command: &mut Command) {
    sanitize_appimage_environment(command);
}

#[cfg(not(windows))]
fn sanitize_appimage_environment(command: &mut Command) {
    // FileFlow Setup peut lui-même être lancé depuis une AppImage. Dans ce cas,
    // le runtime AppImage enrichit LD_LIBRARY_PATH/GIO avec ses bibliothèques.
    // Un binaire système enfant (curl, apt, dnf, pgrep...) ne doit jamais les
    // réutiliser : cela provoque des incompatibilités ABI comme libcurl/nghttp2.
    if std::env::var_os("APPIMAGE").is_none() && std::env::var_os("APPDIR").is_none() {
        return;
    }

    let original_ld = std::env::var_os("APPIMAGE_ORIGINAL_LD_LIBRARY_PATH")
        .or_else(|| std::env::var_os("LD_LIBRARY_PATH_ORIG"));
    if let Some(value) = original_ld.filter(|value| !value.is_empty()) {
        command.env("LD_LIBRARY_PATH", value);
    } else {
        command.env_remove("LD_LIBRARY_PATH");
    }

    for variable in [
        "LD_PRELOAD",
        "GIO_EXTRA_MODULES",
        "GI_TYPELIB_PATH",
        "GTK_PATH",
        "GDK_PIXBUF_MODULE_FILE",
        "GST_PLUGIN_PATH",
        "GST_PLUGIN_SYSTEM_PATH",
        "QT_PLUGIN_PATH",
        "QML2_IMPORT_PATH",
        "PYTHONHOME",
        "PYTHONPATH",
    ] {
        command.env_remove(variable);
    }
    for variable in ["APPDIR", "APPIMAGE", "ARGV0", "OWD"] {
        command.env_remove(variable);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_redaction_masks_known_secrets() {
        assert!(!redact_line("token ghp_123456 password=hunter2").contains("hunter2"));
        assert!(redact_line("normal progress line").contains("progress"));
    }

    #[test]
    fn runtime_logs_expose_resource_progress_without_guessing_plain_output() {
        assert_eq!(
            runtime_log_status("[TRY] winget:Gyan.FFmpeg"),
            Some(("running", "winget:Gyan.FFmpeg"))
        );
        assert_eq!(
            runtime_log_status("[OK]   FFmpeg already available"),
            Some(("ready", "FFmpeg already available"))
        );
        assert_eq!(runtime_log_status("ordinary subprocess output"), None);
    }

    #[test]
    fn output_paths_are_never_part_of_removable_data() {
        let paths = removable_data_paths(Platform::Macos);
        let all = paths
            .cache
            .iter()
            .chain(paths.settings.iter())
            .chain(paths.history.iter())
            .map(|path| path.to_string_lossy().to_ascii_lowercase())
            .collect::<Vec<_>>();
        assert!(all.iter().all(|path| !path.contains("downloads")));
        assert!(all.iter().all(|path| !path.contains("documents")));
    }

    #[test]
    fn compares_release_versions_without_lexicographic_errors() {
        assert!(version_tuple("1.10.0") > version_tuple("1.9.9"));
        assert_eq!(version_tuple("2.0.0-beta.1"), (2, 0, 0));
    }

    #[test]
    fn package_report_preserves_exact_manager_and_package() {
        let path = std::env::temp_dir().join(format!(
            "fileflow-package-report-{}-{}.tsv",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::write(
            &path,
            "zstd\tapt\tzstd\tengine\nruntime:libgtk-3-0\tapt\tlibgtk-3-0\tintegration\n",
        )
        .unwrap();
        let records = read_package_report(&path).unwrap();
        let _ = std::fs::remove_file(path);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].manager, "apt");
        assert_eq!(records[0].package, "zstd");
        assert!(records[1].integration);
    }

    #[test]
    fn removal_specs_use_native_purge_commands() {
        let (program, args, elevated) = base_removal_command("apt", "zstd").unwrap();
        assert_eq!(program, "apt-get");
        assert_eq!(args, ["purge", "-y", "zstd"]);
        assert!(elevated);

        let (program, args, elevated) = base_removal_command("brew-cask", "libreoffice").unwrap();
        assert_eq!(program, "brew");
        assert_eq!(args, ["uninstall", "--cask", "libreoffice"]);
        assert!(!elevated);
    }

    #[test]
    fn package_report_rejects_shell_metacharacters() {
        assert!(!safe_package_token("zstd;touch-/tmp/pwned"));
        assert!(!safe_package_token("$(whoami)"));
    }
}
