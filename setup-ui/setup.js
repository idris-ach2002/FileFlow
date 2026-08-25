(() => {
  'use strict';

  const engineLabels = {
    ffmpeg: 'FFmpeg', vips: 'libvips', imagemagick: 'ImageMagick', qpdf: 'qpdf',
    img2pdf: 'img2pdf', poppler: 'Poppler', ghostscript: 'Ghostscript',
    tesseract: 'Tesseract', ocrmypdf: 'OCRmyPDF', libreoffice: 'LibreOffice',
    pandoc: 'Pandoc', browser: 'Navigateur PDF', exiftool: 'ExifTool', sevenzip: '7-Zip',
    zstd: 'Zstandard', lz4: 'LZ4'
  };

  const views = ['welcome-view', 'custom-view', 'plan-view', 'progress-view', 'finish-view'];
  const state = {
    snapshot: null,
    mode: 'install',
    profile: 'standard',
    plan: null,
    selectedEngines: new Set(),
    stepStates: new Map(),
    stepFractions: new Map(),
    resourceStates: new Map(),
    events: [],
    operationId: null,
    unlisten: null,
    demoTimer: null
  };

  const byId = (id) => document.getElementById(id);
  const engineSelection = window.FileFlowEngineSelection;
  const tauri = window.__TAURI__;
  const invoke = tauri?.core?.invoke;
  const listen = tauri?.event?.listen;

  function showView(id) {
    for (const view of views) byId(view).classList.toggle('hidden', view !== id);
  }

  function platformLabel(platform, architecture) {
    const os = { macos: 'macOS', windows: 'Windows', linux: 'Linux' }[platform] || platform;
    const arch = architecture === 'aarch64' ? 'Apple Silicon / ARM64' : 'Intel / x64';
    return `${os} · ${arch}`;
  }

  async function probe() {
    try {
      const launchMode = invoke ? await invoke('setup_context') : new URLSearchParams(location.search).get('mode');
      state.snapshot = invoke ? await invoke('setup_probe') : demoSnapshot();
      byId('remove-preexisting-engines').checked = false;
      state.mode = state.snapshot.application.installed ? 'repair' : 'install';
      renderProbe();
      if (invoke) await invoke('setup_smoke_ready');
      if (launchMode === 'uninstall' && state.snapshot.application.installed) {
        openCustomize('uninstall');
      } else if (launchMode === 'doctor') {
        await reviewPlan({
          ...requestFromSelection(),
          mode: 'doctor',
          profile: 'standard',
          launchAfter: false
        });
      }
    } catch (error) {
      showToast(`Diagnostic impossible : ${String(error)}`);
      byId('machine-pill').textContent = 'Diagnostic indisponible';
    }
  }

  function renderProbe() {
    const snapshot = state.snapshot;
    byId('machine-pill').textContent = platformLabel(snapshot.platform, snapshot.architecture);
    const installed = snapshot.application.installed;
    byId('primary-label').textContent = installed ? 'Réparer et vérifier FileFlow' : 'Installer FileFlow';
    byId('hero-lead').textContent = installed
      ? `FileFlow ${snapshot.application.version || ''} est installé. Vérifiez l’application, complétez les moteurs ou lancez une réparation guidée.`
      : 'FileFlow vérifie votre appareil, installe uniquement ce qui manque et contrôle le résultat avant de se déclarer prêt.';
    byId('primary-action').disabled = false;
    byId('customize-action').disabled = false;
    byId('customize-action').textContent = installed ? 'Maintenance et désinstallation' : 'Personnaliser';
    renderEnginePicker();
  }

  function renderEnginePicker() {
    const grid = byId('engine-grid');
    grid.replaceChildren();
    for (const engine of state.snapshot.engines) {
      const label = document.createElement('label');
      label.className = `engine-option${engine.installed ? ' detected' : ''}`;
      const checkbox = document.createElement('input');
      checkbox.type = 'checkbox';
      checkbox.value = engine.id;
      checkbox.checked = state.selectedEngines.has(engine.id);
      checkbox.addEventListener('change', () => {
        if (checkbox.checked) state.selectedEngines.add(engine.id);
        else state.selectedEngines.delete(engine.id);
        updateEngineSelectionMeta();
      });
      const text = document.createElement('span');
      const expertRemoval = state.mode === 'uninstall' && byId('remove-preexisting-engines').checked;
      const expertEligible = engine.installed && !engine.installedByFileflow;
      text.textContent = expertRemoval
        ? `${engine.label}${engine.installedByFileflow ? ' · possédé par FileFlow' : engine.installed ? ' · préexistant' : ' · absent'}`
        : `${engine.label}${engine.installed ? ' · prêt à vérifier' : ' · à installer'}`;
      checkbox.disabled = expertRemoval && !expertEligible;
      label.append(checkbox, text);
      grid.append(label);
    }
    updateEngineSelectionMeta();
  }

  function updateEngineSelectionMeta() {
    const engines = state.snapshot?.engines || [];
    const status = engineSelection.summarize(engines, state.selectedEngines);
    const expertRemoval = state.mode === 'uninstall' && byId('remove-preexisting-engines').checked;
    const eligible = expertRemoval
      ? engines.filter((engine) => engine.installed && !engine.installedByFileflow)
      : engines;
    const eligibleSelected = eligible.filter((engine) => state.selectedEngines.has(engine.id)).length;
    const allEligibleSelected = eligible.length > 0 && eligibleSelected === eligible.length;
    byId('engine-selection-summary').textContent = expertRemoval
      ? `${eligibleSelected}/${eligible.length} moteurs présents sélectionnés pour retrait expert`
      : `${status.selected}/${status.total} sélectionnés · ${status.missing} à installer`;
    byId('toggle-all-engines').textContent = (expertRemoval ? allEligibleSelected : status.allSelected)
      ? 'Tout désélectionner'
      : 'Tout sélectionner';
  }

  function openCustomize(forceMode) {
    showView('custom-view');
    const installed = state.snapshot.application.installed;
    const choices = byId('install-choices');
    let uninstall = choices.querySelector('[data-mode="uninstall"]');
    if (installed && !uninstall) {
      uninstall = document.createElement('button');
      uninstall.type = 'button';
      uninstall.className = 'choice-card';
      uninstall.dataset.mode = 'uninstall';
      uninstall.setAttribute('aria-pressed', 'false');
      uninstall.innerHTML = '<span class="choice-icon">↺</span><strong>Désinstaller FileFlow</strong><small>Guidé, vérifié et sans toucher à vos résultats.</small>';
      choices.append(uninstall);
      bindChoice(uninstall);
    }
    if (forceMode === 'uninstall') {
      selectChoice(uninstall);
    } else {
      const selected = choices.querySelector('.choice-card.selected') || choices.querySelector('.choice-card');
      selectChoice(selected);
    }
  }

  function bindChoice(button) {
    button.addEventListener('click', () => selectChoice(button));
  }

  function selectChoice(button) {
    if (!button) return;
    document.querySelectorAll('.choice-card').forEach((candidate) => {
      const selected = candidate === button;
      candidate.classList.toggle('selected', selected);
      candidate.setAttribute('aria-pressed', String(selected));
    });
    if (button.dataset.mode === 'uninstall') {
      state.mode = 'uninstall';
      state.profile = 'full-removal';
      byId('uninstall-options').classList.remove('hidden');
      state.selectedEngines = new Set();
      renderEnginePicker();
      byId('engine-picker').classList.toggle('hidden', !byId('remove-preexisting-engines').checked);
      byId('review-action').firstChild.textContent = 'Examiner la désinstallation ';
      return;
    }
    byId('remove-preexisting-engines').checked = false;
    state.mode = state.snapshot.application.installed ? 'repair' : 'install';
    state.profile = button.dataset.profile || 'standard';
    if (state.profile === 'engines-only' && state.selectedEngines.size === 0) {
      state.selectedEngines = new Set(engineSelection.selectMissingByDefault(
        state.snapshot.engines,
        state.selectedEngines,
      ));
      renderEnginePicker();
    }
    byId('uninstall-options').classList.add('hidden');
    byId('engine-picker').classList.toggle('hidden', !['custom', 'engines-only'].includes(state.profile));
    byId('review-action').firstChild.textContent = 'Examiner le plan ';
  }

  function requestFromSelection() {
    return {
      mode: state.mode,
      profile: state.profile,
      selectedEngines: [...state.selectedEngines],
      removeOwnedEngines: byId('remove-engines').checked,
      removePreexistingEngines: byId('remove-preexisting-engines').checked,
      removeSettings: byId('remove-settings').checked,
      removeHistory: byId('remove-history').checked,
      removeCache: byId('remove-cache').checked,
      preserveOutputs: true,
      launchAfter: state.profile !== 'engines-only',
      dryRun: false
    };
  }

  async function reviewPlan(request = requestFromSelection()) {
    try {
      state.plan = invoke ? await invoke('setup_plan', { request }) : demoPlan(request);
      renderPlan();
      showView('plan-view');
    } catch (error) {
      showToast(`Plan impossible : ${String(error)}`);
    }
  }

  function renderPlan() {
    const enginesOnly = state.plan.request.profile === 'engines-only';
    const list = byId('plan-list');
    list.replaceChildren(...state.plan.steps.map((step, index) => {
      const item = document.createElement('li');
      item.className = 'plan-item';
      const elevation = step.requiresElevation ? 'Autorisation requise' : 'Sans élévation globale';
      item.innerHTML = `<span class="plan-item-index">${index + 1}</span><span><strong></strong><small></small></span><em></em>`;
      item.querySelector('strong').textContent = step.title;
      item.querySelector('small').textContent = step.description;
      item.querySelector('em').textContent = elevation;
      return item;
    }));
    byId('plan-title').textContent = state.mode === 'uninstall'
      ? 'Une désinstallation claire et récupérable.'
      : enginesOnly
        ? 'Les moteurs, sans toucher à FileFlow.'
      : 'Voici exactement ce qui va se passer.';
    byId('plan-summary-title').textContent = state.mode === 'uninstall'
      ? 'Vos résultats restent protégés'
      : enginesOnly ? 'Application conservée' : 'Plan sécurisé';
    byId('plan-summary-copy').textContent = state.mode === 'uninstall'
      ? 'Seuls l’application et les catégories explicitement choisies seront retirés.'
      : enginesOnly
        ? 'Aucune release FileFlow ne sera téléchargée. Seuls les moteurs choisis seront vérifiés ou installés.'
      : 'Aucune modification ne commence avant votre confirmation.';
    const warnings = byId('plan-warnings');
    warnings.replaceChildren(...state.plan.warnings.map((warning) => {
      const item = document.createElement('li'); item.textContent = warning; return item;
    }));
    warnings.classList.toggle('hidden', state.plan.warnings.length === 0);
    byId('start-action').textContent = state.mode === 'uninstall'
      ? 'Confirmer la désinstallation'
      : enginesOnly ? 'Gérer les moteurs' : 'Confirmer et commencer';
  }

  async function startPlan() {
    state.plan.request.dryRun = byId('dry-run').checked;
    state.stepStates = new Map(state.plan.steps.map((step) => [step.id, 'queued']));
    state.stepFractions.clear();
    state.resourceStates.clear();
    state.events = [];
    renderProgressSkeleton();
    showView('progress-view');
    try {
      if (listen) {
        if (state.unlisten) state.unlisten();
        state.unlisten = await listen('fileflow://setup-event', ({ payload }) => handleEvent(payload));
      }
      if (invoke) {
        state.operationId = await invoke('setup_start', { plan: state.plan });
      } else {
        runDemoOperation();
      }
    } catch (error) {
      handleFatal(String(error));
    }
  }

  function renderProgressSkeleton() {
    const enginesOnly = state.plan.request.profile === 'engines-only';
    byId('progress-eyebrow').textContent = state.mode === 'uninstall'
      ? 'DÉSINSTALLATION EN COURS'
      : enginesOnly ? 'MOTEURS EN COURS' : 'INSTALLATION EN COURS';
    byId('progress-title').textContent = state.mode === 'uninstall'
      ? 'Retrait propre de FileFlow'
      : enginesOnly ? 'Préparation des moteurs locaux' : 'Préparation de FileFlow';
    byId('progress-message').textContent = 'Initialisation du moteur transactionnel…';
    byId('progress-number').textContent = '0%';
    byId('global-progress-bar').style.width = '0%';
    byId('terminal-output').replaceChildren();
    byId('resource-status').replaceChildren();
    byId('resource-status').classList.add('hidden');
    const list = byId('step-list');
    list.replaceChildren(...state.plan.steps.map((step) => {
      const item = document.createElement('li');
      item.className = 'step-item';
      item.dataset.stepId = step.id;
      item.innerHTML = '<span class="step-dot">·</span><strong></strong><small>En attente</small>';
      item.querySelector('strong').textContent = step.title;
      return item;
    }));
  }

  function handleEvent(event) {
    if (state.operationId && event.operationId !== state.operationId) return;
    state.events.push(event);
    if (state.events.length > 300) state.events.shift();
    appendTerminal(event);
    if (event.stepId) {
      if (event.eventType === 'step-started') state.stepStates.set(event.stepId, 'active');
      if (event.eventType === 'step-completed') {
        state.stepStates.set(event.stepId, 'done');
        state.stepFractions.set(event.stepId, 1);
      }
      if (event.eventType === 'step-failed') state.stepStates.set(event.stepId, 'failed');
      if (event.eventType === 'bytes-progress' && event.total) {
        state.stepFractions.set(event.stepId, Math.min(1, event.completed / event.total));
        byId('metric-primary').textContent = `${formatBytes(event.completed)} / ${formatBytes(event.total)}`;
        byId('metric-secondary').textContent = 'Progression mesurée sur le fichier réel';
      }
      if (event.eventType === 'resource-progress' && event.detail?.resource) {
        state.resourceStates.set(event.detail.resource, event.detail.status || 'running');
        byId('metric-primary').textContent = event.detail.resource;
        byId('metric-secondary').textContent = resourceStatusLabel(event.detail.status);
        renderResourceStates();
      }
    }
    if (event.message) byId('progress-message').textContent = event.message;
    renderStepStates();
    renderOverallProgress();
    if (event.eventType === 'operation-completed') finish(true);
    if (event.eventType === 'operation-error') handleFatal(event.message);
    if (event.eventType === 'operation-cancelled') finish(false, 'Opération annulée proprement.');
  }

  function resourceStatusLabel(status) {
    return ({
      running: 'Traitement en cours', authorizing: 'Autorisation administrateur', ready: 'Disponible / vérifié',
      missing: 'Indisponible', skipped: 'Ignoré', warning: 'Attention requise'
    })[status] || 'État mis à jour';
  }

  function renderResourceStates() {
    const panel = byId('resource-status');
    const entries = [...state.resourceStates.entries()].slice(-5).reverse();
    panel.replaceChildren(...entries.map(([resource, status]) => {
      const row = document.createElement('div');
      row.className = `resource-row ${status}`;
      const mark = document.createElement('span');
      mark.textContent = status === 'ready' ? '✓' : status === 'missing' ? '×' : status === 'warning' ? '!' : '•';
      const text = document.createElement('span');
      text.textContent = resource;
      row.append(mark, text);
      return row;
    }));
    panel.classList.toggle('hidden', entries.length === 0);
  }

  function renderStepStates() {
    document.querySelectorAll('.step-item').forEach((item) => {
      const status = state.stepStates.get(item.dataset.stepId) || 'queued';
      item.classList.toggle('active', status === 'active');
      item.classList.toggle('done', status === 'done');
      item.classList.toggle('failed', status === 'failed');
      const dot = item.querySelector('.step-dot');
      const copy = item.querySelector('small');
      if (status === 'done') { dot.textContent = '✓'; copy.textContent = 'Validé'; }
      else if (status === 'active') { dot.textContent = '●'; copy.textContent = 'En cours'; }
      else if (status === 'failed') { dot.textContent = '×'; copy.textContent = 'Échec'; }
      else { dot.textContent = '·'; copy.textContent = 'En attente'; }
    });
  }

  function renderOverallProgress() {
    const total = state.plan.totalWeight || state.plan.steps.reduce((sum, step) => sum + step.weight, 0);
    const completed = state.plan.steps.reduce((sum, step) => {
      const status = state.stepStates.get(step.id);
      if (status === 'done') return sum + step.weight;
      if (status === 'active') return sum + step.weight * (state.stepFractions.get(step.id) || 0.12);
      return sum;
    }, 0);
    const percent = Math.max(0, Math.min(100, total ? completed / total * 100 : 0));
    byId('progress-number').textContent = `${Math.round(percent)}%`;
    byId('global-progress-bar').style.width = `${percent}%`;
  }

  function appendTerminal(event) {
    const output = byId('terminal-output');
    const line = document.createElement('div');
    line.className = `terminal-line ${event.level || 'info'}`;
    const time = document.createElement('time');
    time.textContent = new Date(event.timestamp || Date.now()).toLocaleTimeString('fr-FR', { hour: '2-digit', minute: '2-digit', second: '2-digit' });
    const text = document.createElement('span');
    text.textContent = event.message || event.eventType;
    line.append(time, text);
    output.append(line);
    while (output.children.length > 150) output.firstElementChild.remove();
    output.scrollTop = output.scrollHeight;
  }

  function finish(success, message) {
    if (state.unlisten) { state.unlisten(); state.unlisten = null; }
    if (state.demoTimer) { clearInterval(state.demoTimer); state.demoTimer = null; }
    showView('finish-view');
    const uninstall = state.mode === 'uninstall';
    const enginesOnly = state.plan?.request?.profile === 'engines-only';
    const simulation = Boolean(state.plan?.request?.dryRun);
    byId('finish-mark').textContent = success ? '✓' : '!';
    byId('finish-mark').style.background = success ? '' : 'var(--amber)';
    byId('finish-eyebrow').textContent = simulation ? 'SIMULATION TERMINÉE' : success
      ? (uninstall ? 'DÉSINSTALLATION VÉRIFIÉE' : enginesOnly ? 'MOTEURS VÉRIFIÉS' : 'INSTALLATION VÉRIFIÉE')
      : 'OPÉRATION INTERROMPUE';
    byId('finish-title').textContent = simulation ? 'Le plan est exécutable.' : success
      ? (uninstall ? 'FileFlow a été retiré proprement.' : enginesOnly ? 'Les moteurs sélectionnés sont prêts.' : 'FileFlow est prêt.')
      : 'Aucune modification incomplète.';
    byId('finish-copy').textContent = simulation ? 'Toutes les étapes ont été simulées. Aucun fichier ni réglage n’a été modifié.' : message || (uninstall
      ? 'Les éléments sélectionnés ont été retirés. Vos fichiers produits sont restés intacts.'
      : enginesOnly
        ? 'Les moteurs sélectionnés ont été vérifiés sans télécharger ni modifier l’application.'
        : 'L’application et ses moteurs ont passé les contrôles après installation.');
    byId('open-fileflow').classList.toggle('hidden', simulation || uninstall || enginesOnly || !success);
    const done = [...state.stepStates.values()].filter((value) => value === 'done').length;
    byId('finish-stats').innerHTML = `<span>${done} contrôles validés</span><span>Journal enregistré</span><span>Résultats protégés</span>`;
  }

  function handleFatal(message) {
    appendTerminal({ level: 'error', message, timestamp: new Date().toISOString() });
    byId('progress-title').textContent = 'Le traitement n’a pas abouti.';
    byId('progress-message').textContent = message;
    byId('cancel-action').textContent = 'Revenir au plan';
    byId('cancel-action').onclick = () => showView('plan-view');
  }

  async function cancel() {
    byId('cancel-action').disabled = true;
    byId('safe-cancel-copy').textContent = 'Annulation à la prochaine frontière sûre…';
    try {
      if (invoke) await invoke('setup_cancel');
      else finish(false, 'Simulation annulée proprement.');
    } catch (error) {
      showToast(String(error));
    } finally {
      byId('cancel-action').disabled = false;
    }
  }

  function showToast(message) {
    const toast = byId('toast');
    toast.textContent = message;
    toast.classList.remove('hidden');
    setTimeout(() => toast.classList.add('hidden'), 4500);
  }

  function formatBytes(bytes) {
    if (!Number.isFinite(bytes)) return '—';
    const units = ['o', 'Ko', 'Mo', 'Go']; let value = bytes; let index = 0;
    while (value >= 1024 && index < units.length - 1) { value /= 1024; index += 1; }
    return `${value.toFixed(index ? 1 : 0)} ${units[index]}`;
  }

  function demoSnapshot() {
    return {
      platform: navigator.platform.toLowerCase().includes('mac') ? 'macos' : 'linux',
      architecture: 'aarch64',
      application: { installed: false, version: null, path: null, running: false },
      engines: Object.entries(engineLabels).map(([id, label], index) => ({ id, label, installed: index < 5, installedByFileflow: false })),
      warnings: []
    };
  }

  function demoPlan(request) {
    const titles = request.mode === 'uninstall'
      ? ['Diagnostic système', 'Arrêt propre', 'Retrait atomique', 'Données facultatives', 'Post-contrôle', 'Rapport final']
      : ['Diagnostic système', 'Source vérifiée', 'Application atomique', 'Moteurs locaux', 'Centre de maintenance', 'Contrôles après installation', 'Reçu d’installation'];
    const steps = titles.map((title, index) => ({ id: `demo-${index}`, title, description: 'Contrôle réel et journalisé par le moteur FileFlow.', weight: index === 2 ? 35 : 10, requiresElevation: index === 3, interruptible: index !== 2 }));
    return { operationId: crypto.randomUUID(), request, steps, warnings: [], totalWeight: steps.reduce((sum, step) => sum + step.weight, 0) };
  }

  function runDemoOperation() {
    let stepIndex = 0; let fraction = 0;
    state.operationId = state.plan.operationId;
    state.demoTimer = setInterval(() => {
      const step = state.plan.steps[stepIndex];
      if (!step) {
        clearInterval(state.demoTimer); state.demoTimer = null;
        handleEvent({ operationId: state.operationId, eventType: 'operation-completed', level: 'success', message: 'Simulation terminée', timestamp: new Date().toISOString() });
        return;
      }
      if (fraction === 0) handleEvent({ operationId: state.operationId, eventType: 'step-started', stepId: step.id, level: 'info', message: step.title, timestamp: new Date().toISOString() });
      fraction += 0.2;
      handleEvent({ operationId: state.operationId, eventType: 'bytes-progress', stepId: step.id, level: 'info', message: `${step.title} en cours`, completed: Math.round(fraction * 10_000_000), total: 10_000_000, timestamp: new Date().toISOString() });
      if (stepIndex === 3 && fraction <= 0.4) handleEvent({ operationId: state.operationId, eventType: 'resource-progress', stepId: step.id, level: 'info', message: '[TRY] FFmpeg', detail: { resource: 'FFmpeg', status: 'running' }, timestamp: new Date().toISOString() });
      if (fraction >= 1) {
        handleEvent({ operationId: state.operationId, eventType: 'step-completed', stepId: step.id, level: 'success', message: `${step.title} terminé`, timestamp: new Date().toISOString() });
        stepIndex += 1; fraction = 0;
      }
    }, 380);
  }

  document.querySelectorAll('.choice-card').forEach(bindChoice);
  byId('primary-action').addEventListener('click', () => reviewPlan({ ...requestFromSelection(), mode: state.mode, profile: 'standard' }));
  byId('customize-action').addEventListener('click', () => openCustomize());
  byId('custom-back').addEventListener('click', () => showView('welcome-view'));
  byId('review-action').addEventListener('click', () => reviewPlan());
  byId('plan-back').addEventListener('click', () => openCustomize(state.mode === 'uninstall' ? 'uninstall' : undefined));
  byId('start-action').addEventListener('click', startPlan);
  byId('cancel-action').addEventListener('click', cancel);
  byId('terminal-toggle').addEventListener('click', () => {
    const output = byId('terminal-output'); const hidden = output.classList.toggle('hidden');
    byId('terminal-toggle').textContent = hidden ? 'Afficher' : 'Masquer';
    byId('terminal-toggle').setAttribute('aria-expanded', String(!hidden));
  });
  byId('remove-preexisting-engines').addEventListener('change', () => {
    if (state.mode !== 'uninstall') return;
    state.selectedEngines = new Set();
    renderEnginePicker();
    byId('engine-picker').classList.toggle('hidden', !byId('remove-preexisting-engines').checked);
  });
  byId('toggle-all-engines').addEventListener('click', () => {
    if (state.mode === 'uninstall' && byId('remove-preexisting-engines').checked) {
      const eligible = state.snapshot.engines
        .filter((engine) => engine.installed && !engine.installedByFileflow)
        .map((engine) => engine.id);
      const allSelected = eligible.length > 0 && eligible.every((id) => state.selectedEngines.has(id));
      state.selectedEngines = new Set(allSelected ? [] : eligible);
    } else {
      state.selectedEngines = new Set(engineSelection.toggleAll(
        state.snapshot.engines,
        state.selectedEngines,
      ));
    }
    renderEnginePicker();
  });
  byId('open-fileflow').addEventListener('click', async () => {
    try { if (invoke) await invoke('setup_open_fileflow'); else showToast('FileFlow serait ouvert ici.'); }
    catch (error) { showToast(String(error)); }
  });
  byId('close-setup').addEventListener('click', async () => {
    try {
      if (tauri?.window?.getCurrentWindow) await tauri.window.getCurrentWindow().close();
      else window.close();
    } catch (error) { showToast(String(error)); }
  });

  probe();
})();
