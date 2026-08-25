use crate::adapter::SystemSetupAdapter;
use fileflow_setup_core::{
    EventLevel, SetupEvent, SetupMode, SetupProfile, SetupRequest, TransactionEngine, build_plan,
    probe_system,
};
use std::{
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
};

pub fn run_cli() -> i32 {
    match run_cli_inner() {
        Ok(()) => 0,
        Err(message) => {
            eprintln!("[FileFlow Setup] {message}");
            1
        }
    }
}

fn run_cli_inner() -> Result<(), String> {
    let args = std::env::args()
        .skip(1)
        .filter(|argument| argument != "--cli")
        .collect::<Vec<_>>();
    if args
        .iter()
        .any(|argument| matches!(argument.as_str(), "-h" | "--help"))
    {
        print_help();
        return Ok(());
    }
    let command = args
        .iter()
        .find(|argument| !argument.starts_with('-'))
        .map(String::as_str)
        .unwrap_or("install");
    let mode = match command {
        "install" => SetupMode::Install,
        "engines" => SetupMode::Install,
        "repair" => SetupMode::Repair,
        "uninstall" => SetupMode::Uninstall,
        "doctor" => SetupMode::Doctor,
        other => return Err(format!("commande inconnue: {other}")),
    };
    let profile = if command == "engines" {
        SetupProfile::EnginesOnly
    } else if args.iter().any(|argument| argument == "--app-only") {
        SetupProfile::ApplicationOnly
    } else if args.iter().any(|argument| argument == "--full") {
        SetupProfile::FullRemoval
    } else {
        SetupProfile::Standard
    };
    let selected_engines = args
        .windows(2)
        .find(|pair| pair[0] == "--engines")
        .map(|pair| {
            pair[1]
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let remove_preexisting_engines = command == "uninstall"
        && args
            .iter()
            .any(|argument| argument == "--remove-preexisting-engines");
    if remove_preexisting_engines && selected_engines.is_empty() {
        return Err(
            "--remove-preexisting-engines exige --engines id,id afin d’éviter une suppression globale accidentelle"
                .into(),
        );
    }
    let request = SetupRequest {
        mode,
        profile,
        selected_engines,
        remove_owned_engines: command == "uninstall"
            && !args.iter().any(|argument| argument == "--keep-engines"),
        remove_preexisting_engines,
        remove_settings: args.iter().any(|argument| argument == "--remove-settings"),
        remove_history: args.iter().any(|argument| argument == "--remove-history"),
        remove_cache: !args.iter().any(|argument| argument == "--keep-cache"),
        launch_after: command != "engines"
            && !args.iter().any(|argument| argument == "--no-launch"),
        dry_run: args.iter().any(|argument| argument == "--dry-run"),
        ..SetupRequest::default()
    };
    let json = args.iter().any(|argument| argument == "--json");
    let assume_yes = args.iter().any(|argument| argument == "--yes");
    let snapshot = probe_system().map_err(|error| error.to_string())?;
    let plan = build_plan(&snapshot, request);

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&plan).map_err(|error| error.to_string())?
        );
    } else {
        println!(
            "FileFlow Setup — {:?} / {:?}",
            plan.platform, plan.architecture
        );
        for (index, step) in plan.steps.iter().enumerate() {
            println!("  {}. {} — {}", index + 1, step.title, step.description);
        }
        for warning in &plan.warnings {
            println!("  ! {warning}");
        }
    }

    if plan.request.dry_run {
        println!("Simulation terminée : aucune modification effectuée.");
        return Ok(());
    }
    if plan.request.mode != SetupMode::Doctor
        && !assume_yes
        && io::stdin().is_terminal()
        && !confirm("Continuer ? [o/N] ")?
    {
        println!("Opération annulée.");
        return Ok(());
    }

    let operation_dir = snapshot
        .receipt_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("operations")
        .join(plan.operation_id.to_string());
    let resource_dir = setup_resource_dir();
    let adapter = SystemSetupAdapter::new(resource_dir, operation_dir.clone(), snapshot);
    let sink = move |event: SetupEvent| {
        if json {
            if let Ok(value) = serde_json::to_string(&event) {
                println!("{value}");
            }
        } else {
            let symbol = match event.level {
                EventLevel::Success => "✓",
                EventLevel::Warning => "!",
                EventLevel::Error => "✕",
                EventLevel::Info => "·",
            };
            if let Some(percent) = event.progress_percent() {
                println!("{symbol} {:>5.1}% {}", percent, event.message);
            } else {
                println!("{symbol} {}", event.message);
            }
        }
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    runtime.block_on(async {
        TransactionEngine::new(operation_dir.join("journal.json"))
            .execute(&plan, &adapter, &sink, Arc::new(AtomicBool::new(false)))
            .await
            .map_err(|error| error.to_string())
    })?;
    Ok(())
}

fn confirm(prompt: &str) -> Result<bool, String> {
    print!("{prompt}");
    io::stdout().flush().map_err(|error| error.to_string())?;
    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .map_err(|error| error.to_string())?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "o" | "oui" | "y" | "yes"
    ))
}

fn setup_resource_dir() -> PathBuf {
    let executable = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
    if cfg!(target_os = "macos")
        && let Some(contents) = executable
            .ancestors()
            .find(|path| path.file_name().and_then(|name| name.to_str()) == Some("Contents"))
    {
        return contents.join("Resources");
    }
    executable
        .parent()
        .map(|parent| parent.join("resources"))
        .filter(|path| path.exists())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn print_help() {
    println!(
        "FileFlow Setup CLI\n\n\
Usage:\n  fileflow-setup-cli install [--app-only] [--yes]\n  \
fileflow-setup-cli engines [--engines ffmpeg,zstd] [--yes]\n  \
fileflow-setup-cli repair [--yes]\n  \
fileflow-setup-cli uninstall [--keep-engines] [--remove-preexisting-engines --engines id,id] [--remove-settings] [--remove-history] [--yes]\n  \
fileflow-setup-cli doctor [--json]\n\n\
Options:\n  --dry-run      Afficher le plan sans modifier la machine\n  \
--json         Émettre le plan et les événements en JSON\n  \
--no-launch    Ne pas ouvrir FileFlow après installation\n  \
	--engines      Limiter l’opération à une liste de moteurs séparés par des virgules\n  \
	--keep-engines Conserver les moteurs et bibliothèques ajoutés par FileFlow\n  \
--remove-preexisting-engines  Mode expert : retirer uniquement les moteurs préexistants listés avec --engines\n  \
--keep-cache   Conserver le cache lors de la désinstallation"
    );
}
