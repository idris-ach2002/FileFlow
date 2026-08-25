import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { ActivatedRoute, Router } from '@angular/router';
import { AuthStore } from '../../core/auth/auth.store';
import { CapabilityStore } from '../../core/catalog/capability.store';
import { EngineProbe } from '../../core/ipc/tauri.models';
import { TauriBridgeService } from '../../core/ipc/tauri-bridge.service';
import { PreferencesService } from '../../core/preferences/preferences.service';
import { UpdateService } from '../../core/update/update.service';

type SettingsSection = 'general' | 'appearance' | 'files' | 'security' | 'account' | 'performance' | 'updates' | 'engines';

@Component({
  selector: 'ff-settings-page',
  template: `
    <div class="settings-shell">
      <header class="settings-head"><div><p class="ff-kicker">PARAMÈTRES</p><h1>À votre façon.</h1><p>Les réglages importants sont expliqués simplement. Les détails techniques sont rangés à part.</p></div><button class="reset" type="button" (click)="prefs.reset()">Réglages recommandés</button></header>

      <div class="settings-layout">
        <nav class="settings-nav" aria-label="Catégories de paramètres">
          @for (item of sections; track item.id) { <button type="button" [class.active]="section() === item.id" (click)="selectSection(item.id)"><span>{{ item.icon }}</span><span><strong>{{ item.title }}</strong><small>{{ item.subtitle }}</small></span></button> }
        </nav>

        <main class="settings-content">
          @switch (section()) {
            @case ('general') {
              <section class="panel"><header><h2>Utilisation</h2><p>Adaptez la quantité d’aide que FileFlow vous montre.</p></header>
                <label class="switch-row"><span><strong>Mode guidé</strong><small>Explique chaque étape et masque les options techniques inutiles.</small></span><input type="checkbox" [checked]="prefs.beginnerMode()" (change)="prefs.beginnerMode.set(!prefs.beginnerMode())" /></label>
                <label class="switch-row"><span><strong>Notification quand c’est terminé</strong><small>Pratique pour les traitements qui prennent du temps.</small></span><input type="checkbox" [checked]="prefs.notifyOnCompletion()" (change)="prefs.notifyOnCompletion.set(!prefs.notifyOnCompletion())" /></label>
                <label class="switch-row"><span><strong>Ouvrir automatiquement le résultat</strong><small>À activer si vous voulez vérifier chaque résultat immédiatement.</small></span><input type="checkbox" [checked]="prefs.autoOpenResults()" (change)="prefs.autoOpenResults.set(!prefs.autoOpenResults())" /></label>
                <div class="setting-line"><div><strong>Qualité par défaut</strong><small>Vous pourrez toujours la changer avant une action.</small></div><div class="segmented"><button [class.active]="prefs.defaultQuality() === 'small'" (click)="prefs.defaultQuality.set('small')">Plus léger</button><button [class.active]="prefs.defaultQuality() === 'balanced'" (click)="prefs.defaultQuality.set('balanced')">Équilibré</button><button [class.active]="prefs.defaultQuality() === 'high'" (click)="prefs.defaultQuality.set('high')">Haute qualité</button></div></div>
                <div class="setting-line"><div><strong>Langue</strong><small>FileFlow est entièrement disponible en français. Les autres langues arriveront quand toute l’interface et l’aide seront traduites.</small></div><select #lang [value]="prefs.language()" (change)="setLanguage(lang.value)"><option value="fr">Français</option><option value="en" disabled>English — bientôt</option><option value="de" disabled>Deutsch — bientôt</option></select></div>
              </section>
            }
            @case ('appearance') {
              <section class="panel"><header><h2>Affichage & accessibilité</h2><p>Plus grand, plus compact, plus calme : à vous de choisir.</p></header>
                <div class="setting-line"><div><strong>Thème</strong><small>Suit macOS/Linux ou utilise un thème fixe.</small></div><div class="segmented"><button [class.active]="prefs.theme() === 'system'" (click)="prefs.theme.set('system')">Système</button><button [class.active]="prefs.theme() === 'light'" (click)="prefs.theme.set('light')">Clair</button><button [class.active]="prefs.theme() === 'dark'" (click)="prefs.theme.set('dark')">Sombre</button></div></div>
                <div class="setting-line"><div><strong>Taille de l’interface</strong><small>{{ (prefs.uiScale() * 100).toFixed(0) }} %</small></div><div class="zoom-control"><button type="button" (click)="adjustZoom(-.1)">−</button><input type="range" min="0.8" max="1.4" step="0.1" [value]="prefs.uiScale()" (input)="setZoom(zoom.value)" #zoom /><button type="button" (click)="adjustZoom(.1)">＋</button></div></div>
                <div class="setting-line"><div><strong>Densité</strong><small>Confort est recommandé pour la plupart des utilisateurs.</small></div><div class="segmented"><button [class.active]="prefs.density() === 'comfortable'" (click)="prefs.density.set('comfortable')">Confort</button><button [class.active]="prefs.density() === 'compact'" (click)="prefs.density.set('compact')">Compact</button></div></div>
                <label class="switch-row"><span><strong>Réduire les animations</strong><small>Diminue les mouvements et transitions de l’interface.</small></span><input type="checkbox" [checked]="prefs.reduceMotion()" (change)="prefs.reduceMotion.set(!prefs.reduceMotion())" /></label>
              </section>
            }
            @case ('files') {
              <section class="panel"><header><h2>Fichiers & stockage</h2><p>Décidez où FileFlow place les résultats et comment il parcourt vos dossiers.</p></header>
                <div class="storage-card"><span class="folder">▰</span><div><strong>Dossier FileFlow</strong><small>{{ auth.onboarding()?.storageDirectory || 'Aucun dossier défini' }}</small></div><button type="button" (click)="changeStorage()">Modifier</button></div>
                <div class="setting-line"><div><strong>Destination par défaut</strong><small>Le sous-dossier FileFlow évite de mélanger source et résultat.</small></div><div class="segmented"><button [class.active]="prefs.destination() === 'subfolder'" (click)="prefs.destination.set('subfolder')">Sous-dossier</button><button [class.active]="prefs.destination() === 'sameFolder'" (click)="prefs.destination.set('sameFolder')">Même dossier</button><button [class.active]="prefs.destination() === 'ask'" (click)="prefs.destination.set('ask')">Demander</button></div></div>
                <label class="switch-row"><span><strong>Conserver l’arborescence</strong><small>Reproduit les sous-dossiers lors d’un traitement massif.</small></span><input type="checkbox" [checked]="prefs.preserveTree()" (change)="prefs.preserveTree.set(!prefs.preserveTree())" /></label>
                <label class="switch-row"><span><strong>Voir les fichiers cachés</strong><small>Ils restent filtrables dans le Workspace.</small></span><input type="checkbox" [checked]="prefs.showHidden()" (change)="prefs.showHidden.set(!prefs.showHidden())" /></label>
                <div class="safety"><span>✓</span><div><strong>Protection des originaux active</strong><p>FileFlow produit des sorties séparées et refuse l’écrasement silencieux.</p></div></div>
              </section>
            }
            @case ('security') {
              <section class="panel"><header><h2>Confidentialité & sécurité</h2><p>Les choix qui protègent vos fichiers et votre session.</p></header>
                <label class="switch-row"><span><strong>Confirmer les actions sensibles</strong><small>Demandé avant suppression de métadonnées, renommage ou organisation en masse.</small></span><input type="checkbox" [checked]="prefs.confirmDestructive()" (change)="prefs.confirmDestructive.set(!prefs.confirmDestructive())" /></label>
                <div class="security-grid"><article><span>⌂</span><strong>Traitement local</strong><p>Les conversions utilisent les moteurs installés sur cet appareil.</p></article><article><span>◆</span><strong>Mot de passe dérivé</strong><p>PBKDF2-HMAC-SHA256 avec sel unique. Aucun mot de passe en clair dans SQLite.</p></article><article><span>⏱</span><strong>Session temporaire</strong><p>Le token reste en mémoire et expire automatiquement.</p></article><article><span>◎</span><strong>Sorties contrôlées</strong><p>Les commandes post-traitement n’ouvrent que les sorties enregistrées par FileFlow.</p></article></div>
                <div class="password-panel"><div><strong>Changer mon mot de passe</strong><p>Le changement invalide l’ancien token et ouvre une nouvelle session sécurisée.</p></div><div class="password-fields"><input #currentPassword type="password" autocomplete="current-password" placeholder="Mot de passe actuel" /><input #newPassword type="password" autocomplete="new-password" placeholder="Nouveau mot de passe (12 caractères min.)" /><input #confirmPassword type="password" autocomplete="new-password" placeholder="Confirmer le nouveau mot de passe" /></div><button class="ff-button secondary" type="button" (click)="changePassword(currentPassword.value,newPassword.value,confirmPassword.value)">Mettre à jour</button>@if(passwordStatus()){<small class="password-status">{{ passwordStatus() }}</small>}</div>
                <div class="info-note"><strong>À propos du compte FileFlow</strong><p>Ce profil est actuellement sécurisé sur cet appareil. La synchronisation cloud nécessitera un service d’identité distant dédié ; FileFlow ne simule pas un cloud inexistant.</p></div>
              </section>
            }
            @case ('account') {
              <section class="panel"><header><h2>Mon profil</h2><p>Photo, nom et adresse utilisés dans l’application.</p></header>
                <div class="profile-editor">
                  <button class="avatar-editor" type="button" [disabled]="auth.avatarBusy()" (click)="auth.chooseAvatar()">@if(auth.avatarUrl(); as avatar){<img [src]="avatar" alt="Photo de profil" />}@else{<span>{{ auth.initials() }}</span>}<small>{{ auth.avatarBusy() ? 'Sélecteur ouvert…' : 'Changer' }}</small></button>
                  <div><strong>{{ auth.profile()?.displayName }}</strong><p>{{ auth.profile()?.email }}</p><span>Session ouverte jusqu’au {{ sessionExpiry() }}</span></div>
                </div>
                <div class="form-grid"><label><span>Prénom</span><input #first [value]="auth.profile()?.firstName || ''" /></label><label><span>Nom</span><input #last [value]="auth.profile()?.lastName || ''" /></label><label class="full"><span>Nom affiché</span><input #display [value]="auth.profile()?.displayName || ''" /></label><label class="full"><span>E-mail</span><input #email type="email" [value]="auth.profile()?.email || ''" /></label></div>
                <div class="account-actions"><button class="ff-button" type="button" (click)="saveProfile(display.value, first.value, last.value, email.value)">Enregistrer le profil</button><button class="ff-button ghost" type="button" (click)="signOut()">Se déconnecter</button></div>
              </section>
            }
            @case ('performance') {
              <section class="panel"><header><h2>Performances</h2><p>Choisissez la priorité entre silence, équilibre et vitesse.</p></header>
                <div class="performance-options"><button [class.active]="prefs.performanceMode() === 'eco'" (click)="prefs.performanceMode.set('eco')"><span>🌿</span><strong>Éco</strong><small>Moins de CPU et d’I/O en parallèle.</small></button><button [class.active]="prefs.performanceMode() === 'balanced'" (click)="prefs.performanceMode.set('balanced')"><span>⚖</span><strong>Équilibré</strong><small>Recommandé pour garder l’ordinateur fluide.</small></button><button [class.active]="prefs.performanceMode() === 'fast'" (click)="prefs.performanceMode.set('fast')"><span>⚡</span><strong>Rapide</strong><small>Priorité aux conversions lourdes.</small></button></div>
                @if(capabilities.health(); as health){<div class="budget-grid"><div><span>CPU disponible</span><strong>{{ health.scheduler.budget.cpuTokens }}</strong><small>sur {{ health.cpuThreads }} threads logiques</small></div><div><span>Budget mémoire</span><strong>{{ formatMemory(health.scheduler.budget.memoryMb) }}</strong><small>plafond concurrent</small></div><div><span>File I/O</span><strong>{{ health.scheduler.budget.ioTokens }}</strong><small>opérations simultanées</small></div></div>}
                <div class="info-note"><strong>Anti-saturation</strong><p>FFmpeg, libvips, OCR et archives passent par le scheduler Rust. Un moteur déjà multithreadé reçoit aussi un quota de threads.</p></div>
              </section>
            }
            @case ('updates') {
              <section class="panel"><header><h2>Mises à jour</h2><p>FileFlow vérifie uniquement la dernière publication stable, complète et signée pour votre appareil.</p></header>
                <div class="update-settings-card" [attr.data-state]="updater.state()">
                  <span class="update-orb">{{ updater.available() ? '↓' : updater.state() === 'current' ? '✓' : updater.state() === 'error' || updater.configurationMissing() ? '!' : '↻' }}</span>
                  <div><strong>Version installée {{ updater.currentVersion() ?? '—' }}</strong><small>{{ updater.statusLabel() }}</small>@if (updater.version()) { <b>Version stable {{ updater.version() }}</b> }</div>
                  <button class="ff-button" type="button" [disabled]="updater.busy() || updater.configurationMissing()" (click)="updater.available() ? updater.install() : updater.check(false)">{{ updater.available() ? 'Télécharger et installer' : updater.busy() ? 'Vérification…' : updater.configurationMissing() ? 'Updater non configuré' : 'Rechercher une mise à jour' }}</button>
                </div>
                @if (updater.state() === 'downloading' || updater.state() === 'installing') { <div class="settings-update-progress"><span [style.width.%]="updater.state() === 'installing' ? 100 : updater.progress()"></span><b>{{ updater.statusLabel() }}</b></div> }
                @if (updater.configurationMissing()) { <div class="info-note updater-setup"><strong>Initialisation nécessaire pour ce build</strong><p>Exécutez <code>pnpm run updater:setup</code>, conservez la clé privée hors du dépôt, puis reconstruisez FileFlow. La route de publication utilisera ensuite le manifeste signé de GitHub Releases.</p></div> }
                <div class="info-note"><strong>Canal stable et atomique</strong><p>Une version n’est proposée que lorsque macOS Intel/Apple Silicon, Windows x64 et Linux x64/ARM64 ont tous réussi leurs builds. La signature Tauri est contrôlée avant l’installation et les moteurs locaux ne sont pas remplacés.</p></div>
                <div class="maintenance-card"><div><strong>Réparer ou désinstaller FileFlow</strong><p>Ouvre le centre de maintenance transactionnel. Vos fichiers produits ne sont jamais supprimés automatiquement.</p>@if(maintenanceStatus()){<small>{{ maintenanceStatus() }}</small>}</div><button class="ff-button secondary" type="button" (click)="openMaintenance('repair')">Réparer</button><button class="ff-button danger" type="button" (click)="openMaintenance('uninstall')">Désinstaller…</button></div>
              </section>
            }
            @case ('engines') {
              <section class="panel"><header class="engine-title"><div><h2>Moteurs locaux</h2><p>Outils réellement disponibles pour exécuter les actions.</p></div><button class="ff-button secondary" type="button" (click)="refresh()">Actualiser</button></header>
                <div class="engine-summary"><strong>{{ capabilities.engineReadyCount() }}/{{ capabilities.engines().length }}</strong><span>moteurs prêts</span></div>
                <div class="engine-list">@for(engine of capabilities.engines();track engine.id){<article><span class="dot" [class.off]="!engine.available"></span><div><strong>{{ engine.displayName }}</strong><small>{{ engine.available ? executableLabel(engine) : 'Non détecté — les fonctions associées restent désactivées' }}</small></div><b>{{ profileLabel(engine) }}</b></article>}</div>
                <label class="switch-row"><span><strong>Afficher les détails techniques dans l’interface</strong><small>Chemins moteurs, budgets et états avancés.</small></span><input type="checkbox" [checked]="prefs.showTechnicalDetails()" (change)="prefs.showTechnicalDetails.set(!prefs.showTechnicalDetails())" /></label>
              </section>
            }
          }
        </main>
      </div>
    </div>
  `,
  styles: [`
    :host{display:block}.settings-shell{max-width:1180px;margin:0 auto}.settings-head{display:flex;justify-content:space-between;gap:20px;align-items:end}.settings-head h1{margin:0;font-size:48px;letter-spacing:-.055em}.settings-head>div>p:last-child{margin:9px 0 0;color:var(--text-muted);font-size:12px}.reset{min-height:36px;padding:0 12px;border:1px solid var(--border);border-radius:10px;background:var(--surface-1);color:var(--text-muted);font-size:12px;font-weight:750}.settings-layout{display:grid;grid-template-columns:250px minmax(0,1fr);gap:16px;margin-top:30px;align-items:start}.settings-nav{position:sticky;top:24px;display:grid;gap:4px}.settings-nav button{min-height:58px;display:grid;grid-template-columns:30px 1fr;align-items:center;gap:8px;padding:8px 10px;border:0;border-radius:11px;background:transparent;color:var(--text-muted);text-align:left}.settings-nav button:hover{background:var(--surface-2)}.settings-nav button.active{background:var(--accent-soft);color:var(--accent-strong)}.settings-nav button>span:first-child{font-size:16px;text-align:center}.settings-nav strong,.settings-nav small{display:block}.settings-nav strong{font-size:13px}.settings-nav small{margin-top:2px;color:var(--text-faint);font-size:11px}.settings-content{min-width:0}.panel{padding:20px;border:1px solid var(--border);border-radius:16px;background:var(--surface-1);box-shadow:var(--shadow-sm)}.panel>header{padding-bottom:16px;border-bottom:1px solid var(--border)}.panel h2{margin:0;font-size:24px;letter-spacing:-.035em}.panel header p{margin:6px 0 0;color:var(--text-muted);font-size:12px;line-height:1.55}.setting-line,.switch-row{min-height:72px;display:grid;grid-template-columns:minmax(220px,1fr) auto;align-items:center;gap:16px;border-bottom:1px solid var(--border)}.setting-line>div:first-child strong,.setting-line>div:first-child small,.switch-row span strong,.switch-row span small{display:block}.setting-line strong,.switch-row strong{font-size:13px}.setting-line small,.switch-row small{margin-top:4px;color:var(--text-muted);font-size:11.5px;line-height:1.5}.switch-row{cursor:pointer}.switch-row input{width:37px;height:21px;accent-color:var(--accent)}.segmented{display:flex;padding:3px;border-radius:10px;background:var(--surface-2)}.segmented button{min-height:31px;padding:0 10px;border:0;border-radius:8px;background:transparent;color:var(--text-muted);font-size:11px;font-weight:750}.segmented button.active{background:var(--surface-1);color:var(--text);box-shadow:var(--shadow-sm)}select,input:not([type=checkbox]):not([type=range]){height:39px;padding:0 10px;border:1px solid var(--border);border-radius:10px;background:var(--surface-2);color:var(--text);outline:0}.zoom-control{display:grid;grid-template-columns:32px 160px 32px;gap:7px;align-items:center}.zoom-control button{height:32px;border:1px solid var(--border);border-radius:8px;background:var(--surface-2);color:var(--text)}.storage-card{display:grid;grid-template-columns:42px minmax(0,1fr) auto;align-items:center;gap:11px;margin:16px 0 3px;padding:12px;border:1px solid var(--border);border-radius:12px;background:var(--bg-elevated)}.folder{width:40px;height:40px;display:grid;place-items:center;border-radius:11px;background:var(--accent-soft);color:var(--accent)}.storage-card strong,.storage-card small{display:block}.storage-card strong{font-size:13px}.storage-card small{overflow:hidden;margin-top:4px;color:var(--text-muted);font-size:11px;text-overflow:ellipsis;white-space:nowrap}.storage-card button{border:0;background:transparent;color:var(--accent);font-size:11px;font-weight:800}.safety,.info-note{display:flex;gap:10px;margin-top:16px;padding:12px;border-radius:11px;background:var(--success-soft);color:var(--success)}.safety strong,.safety p,.info-note strong,.info-note p{margin:0}.safety strong,.info-note strong{font-size:12px}.safety p,.info-note p{margin-top:3px;color:var(--text-muted);font-size:11px;line-height:1.5}.info-note{display:block;background:var(--bg-elevated);color:var(--text)}.security-grid{display:grid;grid-template-columns:repeat(2,1fr);gap:8px;margin-top:16px}.security-grid article{padding:13px;border:1px solid var(--border);border-radius:11px;background:var(--bg-elevated)}.security-grid article>span{color:var(--accent);font-weight:900}.security-grid strong{display:block;margin-top:8px;font-size:12px}.security-grid p{margin:5px 0 0;color:var(--text-muted);font-size:11px;line-height:1.5}.password-panel{display:grid;grid-template-columns:minmax(180px,1fr) minmax(260px,1.5fr) auto;gap:12px;align-items:center;margin-top:16px;padding:14px;border:1px solid var(--border);border-radius:12px;background:var(--bg-elevated)}.password-panel strong{font-size:12px}.password-panel p{margin:5px 0 0;color:var(--text-muted);font-size:11px;line-height:1.45}.password-fields{display:grid;gap:7px}.password-status{grid-column:1/-1;color:var(--text-muted);font-size:11px}.password-status.success{color:var(--success)}.profile-editor{display:grid;grid-template-columns:84px 1fr;gap:15px;align-items:center;margin:18px 0}.avatar-editor{position:relative;width:78px;height:78px;overflow:hidden;border:1px solid var(--border);border-radius:24px;background:var(--accent-soft);color:var(--accent);font-size:20px;font-weight:900}.avatar-editor img{width:100%;height:100%;object-fit:cover}.avatar-editor small{position:absolute;inset:auto 0 0;padding:5px;background:rgb(0 0 0/.55);color:white;font-size:10px}.profile-editor>div>strong{font-size:17px}.profile-editor p,.profile-editor div>span{margin:4px 0 0;color:var(--text-muted);font-size:11px}.form-grid{display:grid;grid-template-columns:repeat(2,1fr);gap:10px}.form-grid label{display:grid;gap:5px;color:var(--text-muted);font-size:11px;font-weight:750}.form-grid .full{grid-column:1/-1}.account-actions{display:flex;gap:8px;margin-top:16px}.performance-options{display:grid;grid-template-columns:repeat(3,1fr);gap:8px;margin-top:16px}.performance-options button{min-height:110px;padding:13px;border:1px solid var(--border);border-radius:12px;background:var(--bg-elevated);color:var(--text);text-align:left}.performance-options button.active{border-color:var(--accent);background:var(--accent-soft)}.performance-options span,.performance-options strong,.performance-options small{display:block}.performance-options span{font-size:19px}.performance-options strong{margin-top:9px;font-size:12px}.performance-options small{margin-top:4px;color:var(--text-muted);font-size:11px;line-height:1.4}.budget-grid{display:grid;grid-template-columns:repeat(3,1fr);gap:8px;margin-top:14px}.budget-grid>div{padding:12px;border-radius:11px;background:var(--surface-2)}.budget-grid span,.budget-grid strong,.budget-grid small{display:block}.budget-grid span{color:var(--text-faint);font-size:10px;text-transform:uppercase;font-weight:850}.budget-grid strong{margin-top:5px;font-size:20px}.budget-grid small{margin-top:3px;color:var(--text-muted);font-size:10px}.engine-title{display:flex;justify-content:space-between;align-items:start}.engine-summary{display:flex;align-items:baseline;gap:7px;margin:16px 0}.engine-summary strong{font-size:32px}.engine-summary span{color:var(--text-muted);font-size:11px}.engine-list{display:grid;grid-template-columns:repeat(2,1fr);border:1px solid var(--border);border-radius:12px;overflow:hidden}.engine-list article{min-height:60px;display:grid;grid-template-columns:10px 1fr auto;align-items:center;gap:8px;padding:9px 11px;border-bottom:1px solid var(--border)}.engine-list article:nth-child(odd){border-right:1px solid var(--border)}.dot{width:7px;height:7px;border-radius:50%;background:var(--success)}.dot.off{background:var(--text-faint)}.engine-list strong,.engine-list small{display:block}.engine-list strong{font-size:11px}.engine-list small{margin-top:3px;color:var(--text-faint);font-size:10px}.engine-list b{color:var(--text-faint);font-size:9px}@media(max-width:900px){.settings-layout{grid-template-columns:1fr}.settings-nav{position:static;display:flex;overflow:auto;padding-bottom:5px}.settings-nav button{min-width:150px}.security-grid,.engine-list{grid-template-columns:1fr}.engine-list article:nth-child(odd){border-right:0}}@media(max-width:620px){.settings-head h1{font-size:38px}.settings-head .reset{display:none}.setting-line{grid-template-columns:1fr;gap:8px;padding:12px 0}.segmented{width:100%}.segmented button{flex:1}.performance-options,.budget-grid,.form-grid{grid-template-columns:1fr}.form-grid .full{grid-column:auto}}
  `, `
    .update-settings-card{display:grid;grid-template-columns:58px minmax(0,1fr) auto;align-items:center;gap:14px;padding:16px;border:1px solid var(--border);border-radius:17px;background:var(--surface-2)}.update-orb{width:54px;height:54px;display:grid;place-items:center;border-radius:16px;background:var(--accent-soft);color:var(--accent);font-size:24px;font-weight:900}.update-settings-card[data-state=current] .update-orb{background:var(--success-soft);color:var(--success)}.update-settings-card[data-state=error] .update-orb,.update-settings-card[data-state=unavailable] .update-orb{background:var(--danger-soft);color:var(--danger)}.update-settings-card strong,.update-settings-card small,.update-settings-card b{display:block}.update-settings-card strong{font-size:13px}.update-settings-card small{margin-top:4px;color:var(--text-muted);font-size:10px}.update-settings-card b{margin-top:6px;color:var(--accent);font-size:9px}.settings-update-progress{position:relative;height:8px;margin:13px 0 31px;border-radius:99px;background:var(--surface-3)}.settings-update-progress span{display:block;height:100%;border-radius:inherit;background:linear-gradient(90deg,var(--accent),var(--violet));transition:width .2s}.settings-update-progress b{position:absolute;top:13px;left:0;color:var(--text-muted);font-size:9px}.updater-setup{border:1px solid color-mix(in srgb,var(--danger) 25%,var(--border));background:var(--danger-soft)}.updater-setup code{font-weight:800}.maintenance-card{display:grid;grid-template-columns:minmax(0,1fr) auto auto;align-items:center;gap:10px;margin-top:16px;padding:14px;border:1px solid var(--border);border-radius:13px;background:var(--bg-elevated)}.maintenance-card strong,.maintenance-card p,.maintenance-card small{display:block;margin:0}.maintenance-card p{margin-top:4px;color:var(--text-muted);font-size:11px;line-height:1.45}.maintenance-card small{margin-top:6px;color:var(--accent);font-size:10px}.ff-button.danger{background:var(--danger-soft);color:var(--danger)}@media(max-width:700px){.update-settings-card{grid-template-columns:48px 1fr}.update-settings-card .ff-button{grid-column:1/-1}.update-orb{width:46px;height:46px}.maintenance-card{grid-template-columns:1fr 1fr}.maintenance-card>div{grid-column:1/-1}}
  `],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class SettingsPage {
  protected readonly capabilities = inject(CapabilityStore);
  protected readonly prefs = inject(PreferencesService);
  protected readonly auth = inject(AuthStore);
  protected readonly updater = inject(UpdateService);
  private readonly bridge = inject(TauriBridgeService);
  private readonly router = inject(Router);
  private readonly route = inject(ActivatedRoute);
  protected readonly section = signal<SettingsSection>('general');
  protected readonly passwordStatus = signal<string | null>(null);
  protected readonly maintenanceStatus = signal<string | null>(null);
  protected readonly sections: {id:SettingsSection;icon:string;title:string;subtitle:string}[] = [
    {id:'general',icon:'⌂',title:'Utilisation',subtitle:'Aide, langue, comportements'},
    {id:'appearance',icon:'◐',title:'Affichage',subtitle:'Thème, taille, animations'},
    {id:'files',icon:'▰',title:'Fichiers & stockage',subtitle:'Dossiers et résultats'},
    {id:'security',icon:'◇',title:'Sécurité',subtitle:'Confidentialité et confirmations'},
    {id:'account',icon:'●',title:'Compte & profil',subtitle:'Photo et informations'},
    {id:'performance',icon:'⚡',title:'Performances',subtitle:'Éco, équilibre, vitesse'},
    {id:'updates',icon:'↓',title:'Mises à jour',subtitle:'Version stable et installation'},
    {id:'engines',icon:'⚙',title:'Moteurs locaux',subtitle:'Diagnostic avancé'},
  ];

  constructor() {
    this.route.paramMap.pipe(takeUntilDestroyed()).subscribe((parameters) => {
      const requested = parameters.get('section');
      const section = this.sections.find((item) => item.id === requested)?.id;
      if (section) {
        this.section.set(section);
      } else {
        void this.router.navigate(['/settings', 'general'], { replaceUrl: true });
      }
    });
  }

  protected selectSection(section: SettingsSection): void {
    this.section.set(section);
    void this.router.navigate(['/settings', section]);
  }

  protected refresh(): void { void this.capabilities.refreshEngines(); }
  protected executableLabel(engine: EngineProbe): string { return engine.executable?.split('/').pop() ?? 'Disponible'; }
  protected profileLabel(engine: EngineProbe): string { const cpu=engine.resourceProfile.cpuWeight; return cpu>=5?'intensif':cpu>=3?'moyen':'léger'; }
  protected formatMemory(megabytes: number): string { return megabytes>=1024?`${(megabytes/1024).toFixed(megabytes%1024===0?0:1)} Go`:`${megabytes} Mo`; }
  protected adjustZoom(delta:number):void{this.prefs.uiScale.set(Math.min(1.4,Math.max(.8,Math.round((this.prefs.uiScale()+delta)*10)/10)));}
  protected setZoom(value:string):void{const number=Number(value);if(Number.isFinite(number))this.prefs.uiScale.set(number);}
  protected setLanguage(value:string):void{if(value==='fr'||value==='en'||value==='de')this.prefs.language.set(value);}
  protected sessionExpiry():string{const value=this.auth.session()?.expiresAt;if(!value)return'—';return new Intl.DateTimeFormat('fr-FR',{hour:'2-digit',minute:'2-digit'}).format(new Date(value));}
  protected async changeStorage():Promise<void>{const directory=await this.auth.chooseStorageDirectory();if(directory)await this.auth.saveSetup({storageDirectory:directory});}
  protected async saveProfile(displayName:string,firstName:string,lastName:string,email:string):Promise<void>{await this.auth.updateProfile({displayName,firstName,lastName,email});}
  protected async changePassword(currentPassword:string,newPassword:string,confirmPassword:string):Promise<void>{
    this.passwordStatus.set(null);
    if(newPassword!==confirmPassword){this.passwordStatus.set('Les deux nouveaux mots de passe ne correspondent pas.');return;}
    if(newPassword.length<12){this.passwordStatus.set('Utilisez au moins 12 caractères.');return;}
    const ok=await this.auth.changePassword({currentPassword,newPassword});
    this.passwordStatus.set(ok?'Mot de passe modifié. La session a été renouvelée.':this.auth.error()??'Impossible de modifier le mot de passe.');
  }
  protected async signOut():Promise<void>{await this.auth.logout();await this.router.navigate(['/welcome']);}
  protected async openMaintenance(mode:'repair'|'uninstall'):Promise<void>{
    this.maintenanceStatus.set(null);
    try{await this.bridge.launchFileFlowSetup(mode);this.maintenanceStatus.set('FileFlow Setup a été ouvert dans une fenêtre séparée.');}
    catch(error){this.maintenanceStatus.set(String(error));}
  }
}
