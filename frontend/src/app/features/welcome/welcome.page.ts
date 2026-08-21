import { ChangeDetectionStrategy, Component, computed, effect, inject, signal } from '@angular/core';
import { Router } from '@angular/router';
import { AuthStore } from '../../core/auth/auth.store';
import { AccountProfile } from '../../core/ipc/tauri.models';
import { PreferencesService } from '../../core/preferences/preferences.service';
import { UiMemoryService } from '../../core/state/ui-memory.service';

@Component({
  selector: 'ff-welcome-page',
  template: `
    <main class="welcome-shell">
      <section class="welcome-card">
        <header class="welcome-brand">
          <div class="logo">F</div>
          <div><strong>FileFlow</strong><span>Vos fichiers, simplement.</span></div>
        </header>

        <div class="progress" [attr.aria-label]="'Étape ' + (step() + 1) + ' sur 4'">
          @for (index of [0,1,2,3]; track index) {
            <span [class.active]="index <= step()"></span>
          }
        </div>

        @if (auth.error(); as error) {
          <div class="error-banner" role="alert">
            <strong>Impossible de continuer</strong><span>{{ error }}</span>
            <button type="button" (click)="auth.clearError()">Fermer</button>
          </div>
        }

        @switch (step()) {
          @case (0) {
            <section class="step-panel auth-panel">
              <p class="eyebrow">BIENVENUE</p>
              <h1>{{ mode() === 'create' ? 'Créons votre espace FileFlow.' : 'Heureux de vous revoir.' }}</h1>
              <p class="lead">
                {{ mode() === 'create'
                  ? 'Votre compte reste local à cet appareil. FileFlow mémorise vos préférences sans envoyer vos documents dans le cloud.'
                  : 'Choisissez un compte déjà utilisé sur cet appareil, ou saisissez une autre adresse.' }}
              </p>

              @if (mode() === 'login' && auth.knownAccounts().length) {
                <div class="known-accounts" aria-label="Comptes connus sur cet appareil">
                  @for (account of auth.knownAccounts(); track account.id) {
                    <button type="button" class="account-card" [class.selected]="selectedAccountId() === account.id" (click)="selectAccount(account)">
                      <span class="account-avatar">{{ initials(account) }}</span>
                      <span><strong>{{ account.displayName || account.firstName || account.email }}</strong><small>{{ account.email }}</small></span>
                      <b>{{ selectedAccountId() === account.id ? '✓' : '›' }}</b>
                    </button>
                  }
                </div>
              }

              <div class="mode-switch">
                <button type="button" [class.active]="mode() === 'login'" (click)="switchMode('login')">Se connecter</button>
                <button type="button" [class.active]="mode() === 'create'" (click)="switchMode('create')">Créer un compte</button>
              </div>

              @if (mode() === 'create') {
                <div class="form-grid two">
                  <label><span>Prénom</span><input #first autocomplete="given-name" [value]="firstName()" (input)="firstName.set(first.value)" placeholder="Prénom" /></label>
                  <label><span>Nom</span><input #last autocomplete="family-name" [value]="lastName()" (input)="lastName.set(last.value)" placeholder="Nom" /></label>
                </div>
                <label><span>Nom affiché</span><input #display autocomplete="nickname" [value]="displayName()" (input)="displayName.set(display.value)" placeholder="Comment FileFlow doit vous appeler" /></label>
              }

              <label>
                <span>Adresse e-mail</span>
                <input #email type="email" autocomplete="email" list="known-fileflow-emails" [value]="emailValue()" (input)="emailValue.set(email.value); selectedAccountId.set(null)" placeholder="vous@exemple.fr" />
                <datalist id="known-fileflow-emails">
                  @for (account of auth.knownAccounts(); track account.id) { <option [value]="account.email"></option> }
                </datalist>
              </label>

              <label>
                <span>Mot de passe</span>
                <div class="password-field">
                  <input #password [type]="passwordVisible() ? 'text' : 'password'" [attr.autocomplete]="mode() === 'create' ? 'new-password' : 'current-password'" [value]="passwordValue()" (input)="passwordValue.set(password.value)" (keydown.enter)="submitAuth()" placeholder="Au moins 12 caractères" />
                  <button type="button" class="eye-button" [attr.aria-label]="passwordVisible() ? 'Masquer le mot de passe' : 'Afficher le mot de passe'" (mousedown)="$event.preventDefault()" (click)="passwordVisible.update((visible) => !visible)">{{ passwordVisible() ? '◉' : '◌' }}</button>
                </div>
              </label>

              <label class="remember-row">
                <input type="checkbox" [checked]="rememberDevice()" (change)="rememberDevice.update((value) => !value)" />
                <span><strong>Rester connecté sur cet appareil</strong><small>Au prochain lancement, FileFlow reprend immédiatement votre session. Le mot de passe n’est jamais mémorisé.</small></span>
              </label>

              <button class="primary-action" type="button" [disabled]="busy() || !authFormReady()" (click)="submitAuth()">
                {{ busy() ? 'Vérification sécurisée…' : (mode() === 'create' ? 'Créer mon espace' : 'Continuer') }}
              </button>
              <div class="security-note"><span>◆</span><p><strong>Connexion locale et persistante.</strong> Les comptes connus servent uniquement à vous éviter de ressaisir l’adresse. Les mots de passe ne sont jamais enregistrés en clair ni conservés dans les brouillons.</p></div>
            </section>
          }

          @case (1) {
            <section class="step-panel">
              <p class="eyebrow">ÉTAPE 2 · VOS RÉSULTATS</p>
              <h1>Où voulez-vous retrouver vos fichiers ?</h1>
              <p class="lead">FileFlow conserve les originaux et place les nouveaux résultats dans un endroit prévisible.</p>

              <button class="folder-choice" type="button" [disabled]="folderBusy()" (click)="chooseFolder()">
                <span class="folder-icon">▰</span>
                <span><strong>{{ storageDirectory() ? 'Dossier FileFlow' : 'Choisir un dossier' }}</strong><small>{{ storageDirectory() || 'Sélectionnez l’emplacement qui vous convient.' }}</small></span>
                <b>{{ folderBusy() ? 'Ouverture…' : 'Modifier' }}</b>
              </button>
              <button class="secondary-action" type="button" [disabled]="folderBusy()" (click)="useRecommendedFolder()">Utiliser le dossier recommandé</button>
              @if (folderError()) { <p class="folder-error">{{ folderError() }}</p> }

              <div class="simple-rule"><span>✓</span><div><strong>Originaux intacts</strong><p>Une conversion produit toujours un nouveau résultat. Aucun document n’est remplacé silencieusement.</p></div></div>
              <div class="footer-actions"><button type="button" class="ghost" (click)="step.set(0)">Retour</button><button type="button" class="primary-action inline" [disabled]="!storageDirectory() || folderBusy()" (click)="step.set(2)">Continuer</button></div>
            </section>
          }

          @case (2) {
            <section class="step-panel">
              <p class="eyebrow">ÉTAPE 3 · VOTRE CONFORT</p>
              <h1>FileFlow peut faire simple.</h1>
              <p class="lead">Les options expertes resteront disponibles, mais hors de l’accueil. Ces choix sont modifiables plus tard.</p>

              <label class="choice-row"><input type="checkbox" [checked]="beginnerMode()" (change)="beginnerMode.update((v) => !v)" /><span><strong>Me guider étape par étape</strong><small>Les 5 vues simples restent prioritaires et les détails techniques sont masqués.</small></span></label>
              <label class="choice-row"><input type="checkbox" [checked]="preserveOriginals()" disabled /><span><strong>Toujours conserver mes originaux</strong><small>Les intermédiaires sont créés dans un workspace temporaire puis nettoyés après validation du résultat.</small></span></label>
              <label class="choice-row"><input type="checkbox" [checked]="notifications()" (change)="notifications.update((v) => !v)" /><span><strong>Me prévenir quand un long traitement est terminé</strong><small>Utile pour OCR, gros PDF, vidéos et dossiers complets.</small></span></label>
              <label class="choice-row"><input type="checkbox" [checked]="confirmDestructive()" (change)="confirmDestructive.update((v) => !v)" /><span><strong>Demander avant les actions sensibles</strong><small>Nettoyage de métadonnées, renommage ou organisation en masse.</small></span></label>

              <div class="footer-actions"><button type="button" class="ghost" (click)="step.set(1)">Retour</button><button type="button" class="primary-action inline" (click)="savePreferencesAndContinue()">Continuer</button></div>
            </section>
          }

          @case (3) {
            <section class="step-panel final-panel">
              <div class="success-orb">✓</div>
              <p class="eyebrow">PRÊT</p>
              <h1>Vous n’avez rien à apprendre.</h1>
              <p class="lead">Déposez un fichier ou un dossier. FileFlow détecte le contenu, choisit le meilleur chemin et vous montre uniquement ce qui est utile.</p>
              <div class="tour-grid">
                <article><span>1</span><strong>Ajoutez ce que vous avez</strong><p>Fichier, lot, dossier ou ZIP. Le type réel est vérifié localement.</p></article>
                <article><span>2</span><strong>Choisissez le résultat</strong><p>Convertir, compresser, extraire, organiser, signer ou protéger.</p></article>
                <article><span>3</span><strong>FileFlow finalise</strong><p>Qualité, validation et nettoyage des intermédiaires avant de livrer le résultat.</p></article>
              </div>
              <button class="primary-action" type="button" [disabled]="busy()" (click)="finish()">{{ busy() ? 'Préparation…' : 'Ouvrir FileFlow' }}</button>
              <button class="help-link" type="button" (click)="finish('/help')">Voir d’abord le guide d’utilisation</button>
            </section>
          }
        }
      </section>

      <aside class="welcome-aside">
        <div class="promise"><span>01</span><strong>Démarrage immédiat</strong><p>Une session approuvée est restaurée localement sans refaire le calcul du mot de passe à chaque lancement.</p></div>
        <div class="promise"><span>02</span><strong>Mémoire utile</strong><p>FileFlow mémorise l’étape, l’adresse et les réglages non sensibles. Jamais le mot de passe.</p></div>
        <div class="promise"><span>03</span><strong>Plusieurs comptes</strong><p>Les profils présents sur cet appareil sont proposés comme dans un sélecteur de comptes moderne.</p></div>
      </aside>
    </main>
  `,
  styles: [`
    :host{display:block;min-height:100vh;background:radial-gradient(circle at 18% 10%,color-mix(in srgb,var(--accent) 11%,transparent),transparent 34%),radial-gradient(circle at 84% 82%,color-mix(in srgb,var(--violet) 8%,transparent),transparent 30%),var(--bg);color:var(--text)}
    .welcome-shell{min-height:100vh;display:grid;grid-template-columns:minmax(0,760px) minmax(280px,370px);justify-content:center;align-items:center;gap:34px;padding:36px}.welcome-card{min-height:700px;padding:30px 42px 36px;border:1px solid var(--border);border-radius:30px;background:color-mix(in srgb,var(--surface-1) 97%,transparent);box-shadow:var(--shadow-lg)}
    .welcome-brand{display:flex;align-items:center;gap:12px}.logo{width:44px;height:44px;display:grid;place-items:center;border-radius:14px;background:linear-gradient(145deg,var(--accent),var(--violet));color:white;font-size:18px;font-weight:900;box-shadow:0 10px 26px color-mix(in srgb,var(--accent) 25%,transparent)}.welcome-brand strong,.welcome-brand span{display:block}.welcome-brand strong{font-size:18px}.welcome-brand span{margin-top:2px;color:var(--text-faint);font-size:10px;font-weight:850;letter-spacing:.08em;text-transform:uppercase}
    .progress{display:grid;grid-template-columns:repeat(4,1fr);gap:7px;margin:24px 0 32px}.progress span{height:4px;border-radius:999px;background:var(--border)}.progress span.active{background:linear-gradient(90deg,var(--accent),var(--violet))}.step-panel{max-width:620px;margin:auto}.eyebrow{margin:0 0 10px;color:var(--accent);font-size:11px;font-weight:900;letter-spacing:.1em}.step-panel h1{margin:0;font-size:clamp(42px,4.8vw,58px);line-height:.99;letter-spacing:-.055em}.lead{margin:15px 0 25px;color:var(--text-muted);font-size:15px;line-height:1.65}
    .error-banner{display:grid;grid-template-columns:1fr auto;gap:5px 16px;margin:-12px 0 20px;padding:13px 15px;border:1px solid color-mix(in srgb,var(--danger) 22%,var(--border));border-radius:14px;background:var(--danger-soft);color:var(--danger)}.error-banner span{grid-column:1;font-size:12px}.error-banner button{grid-column:2;grid-row:1/3;border:0;background:transparent;color:inherit;font-weight:800}
    .known-accounts{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:9px;margin:0 0 18px}.account-card{display:grid;grid-template-columns:42px minmax(0,1fr) auto;align-items:center;gap:11px;min-height:66px;padding:10px 12px;border:1px solid var(--border);border-radius:15px;background:var(--surface-2);color:var(--text);text-align:left;transition:var(--transition)}.account-card:hover,.account-card.selected{border-color:color-mix(in srgb,var(--accent) 48%,var(--border));background:var(--accent-soft)}.account-avatar{width:40px;height:40px;display:grid;place-items:center;border-radius:13px;background:linear-gradient(145deg,var(--accent),var(--violet));color:#fff;font-size:12px;font-weight:900}.account-card strong,.account-card small{display:block;overflow:hidden;text-overflow:ellipsis}.account-card strong{font-size:13px}.account-card small{margin-top:3px;color:var(--text-muted);font-size:11px}.account-card b{color:var(--accent)}
    .mode-switch{display:grid;grid-template-columns:1fr 1fr;gap:5px;margin-bottom:16px;padding:4px;border-radius:13px;background:var(--surface-3)}.mode-switch button{min-height:38px;border:0;border-radius:10px;background:transparent;color:var(--text-muted);font-weight:800}.mode-switch button.active{background:var(--surface-1);color:var(--text);box-shadow:var(--shadow-xs)}label{display:block;margin-top:12px}label>span{display:block;margin-bottom:6px;color:var(--text-muted);font-size:12.5px;font-weight:760}.form-grid.two{display:grid;grid-template-columns:1fr 1fr;gap:10px}.form-grid label{margin-top:0}input:not([type=checkbox]){box-sizing:border-box;width:100%;height:49px;padding:0 13px;border-radius:13px;font-size:14px}.password-field{position:relative}.password-field input{padding-right:50px}.eye-button{position:absolute;right:5px;top:5px;width:39px;height:39px;border:0;border-radius:10px;background:transparent;color:var(--text-muted);font-size:18px}.eye-button:hover{background:var(--accent-soft);color:var(--accent)}
    .remember-row,.choice-row{display:flex;align-items:flex-start;gap:11px;padding:13px 14px;border:1px solid var(--border);border-radius:15px;background:var(--surface-2)}.remember-row input,.choice-row input{margin-top:3px}.remember-row span,.choice-row span{margin:0}.remember-row strong,.remember-row small,.choice-row strong,.choice-row small{display:block}.remember-row strong,.choice-row strong{font-size:13px}.remember-row small,.choice-row small{margin-top:4px;color:var(--text-muted);font-size:11.5px;line-height:1.5}
    .primary-action,.secondary-action{width:100%;min-height:52px;margin-top:15px;border:0;border-radius:14px;font-size:14px;font-weight:850}.primary-action{background:linear-gradient(135deg,var(--accent),var(--violet));color:white;box-shadow:0 13px 28px color-mix(in srgb,var(--accent) 24%,transparent)}.primary-action:disabled{opacity:.45;box-shadow:none}.secondary-action{border:1px solid var(--border);background:var(--surface-2);color:var(--text)}.primary-action.inline{width:auto;min-width:150px;margin:0}.security-note,.simple-rule{display:flex;gap:10px;margin-top:14px;padding:12px 13px;border-radius:13px;background:var(--accent-soft);color:var(--text-muted)}.security-note span,.simple-rule>span{color:var(--accent);font-weight:900}.security-note p,.simple-rule p{margin:0;font-size:11.5px;line-height:1.5}.security-note strong,.simple-rule strong{color:var(--text)}
    .folder-choice{width:100%;display:grid;grid-template-columns:48px 1fr auto;align-items:center;gap:13px;padding:14px;border:1px solid var(--border);border-radius:16px;background:var(--surface-2);color:var(--text);text-align:left}.folder-icon{width:44px;height:44px;display:grid;place-items:center;border-radius:13px;background:var(--accent-soft);color:var(--accent)}.folder-choice strong,.folder-choice small{display:block}.folder-choice small{margin-top:3px;color:var(--text-muted);font-size:11.5px}.folder-choice b{color:var(--accent);font-size:12px}.folder-error{color:var(--danger);font-size:12px}.choice-row{margin-top:9px}.footer-actions{display:flex;justify-content:space-between;align-items:center;margin-top:22px}.ghost,.help-link{border:0;background:transparent;color:var(--text-muted);font-weight:750}.final-panel{text-align:center}.success-orb{width:78px;height:78px;display:grid;place-items:center;margin:0 auto 15px;border-radius:50%;background:var(--success-soft);color:var(--success);font-size:34px}.tour-grid{display:grid;grid-template-columns:repeat(3,1fr);gap:9px;margin:20px 0;text-align:left}.tour-grid article{padding:14px;border:1px solid var(--border);border-radius:15px;background:var(--surface-2)}.tour-grid span{width:27px;height:27px;display:grid;place-items:center;margin-bottom:10px;border-radius:9px;background:var(--accent-soft);color:var(--accent);font-size:11px;font-weight:900}.tour-grid strong{display:block;font-size:13px}.tour-grid p{margin:5px 0 0;color:var(--text-muted);font-size:11.5px;line-height:1.45}.help-link{margin-top:12px;color:var(--accent)}
    .welcome-aside{display:grid;gap:12px}.promise{padding:22px;border:1px solid var(--border);border-radius:22px;background:color-mix(in srgb,var(--surface-1) 88%,transparent);box-shadow:var(--shadow-sm)}.promise span{display:inline-grid;place-items:center;width:34px;height:34px;margin-bottom:13px;border-radius:11px;background:var(--accent-soft);color:var(--accent);font-size:11px;font-weight:900}.promise strong{display:block;font-size:18px;letter-spacing:-.025em}.promise p{margin:7px 0 0;color:var(--text-muted);font-size:13px;line-height:1.55}
    @media(max-width:930px){.welcome-shell{grid-template-columns:minmax(0,700px);padding:20px}.welcome-aside{display:none}.welcome-card{min-height:auto}}@media(max-width:600px){.welcome-shell{padding:0}.welcome-card{min-height:100vh;padding:24px 20px;border:0;border-radius:0}.form-grid.two,.tour-grid,.known-accounts{grid-template-columns:1fr}.step-panel h1{font-size:38px}}
  `],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class WelcomePage {
  protected readonly auth = inject(AuthStore);
  private readonly prefs = inject(PreferencesService);
  private readonly router = inject(Router);
  private readonly memory = inject(UiMemoryService);
  private readonly draft = this.memory.welcomeDraft();

  protected readonly step = signal(Math.min(3, Math.max(0, this.draft?.step ?? 0)));
  protected readonly mode = signal<'create' | 'login'>(this.draft?.mode ?? 'create');
  protected readonly selectedAccountId = signal<string | null>(this.draft?.selectedAccountId ?? null);
  protected readonly busy = signal(false);
  protected readonly folderBusy = signal(false);
  protected readonly folderError = signal<string | null>(null);
  protected readonly firstName = signal(this.draft?.firstName ?? '');
  protected readonly lastName = signal(this.draft?.lastName ?? '');
  protected readonly displayName = signal(this.draft?.displayName ?? '');
  protected readonly emailValue = signal(this.draft?.email ?? '');
  protected readonly passwordValue = signal('');
  protected readonly passwordVisible = signal(false);
  protected readonly rememberDevice = signal(true);
  protected readonly storageDirectory = signal('');
  protected readonly beginnerMode = signal(true);
  protected readonly preserveOriginals = signal(true);
  protected readonly notifications = signal(true);
  protected readonly confirmDestructive = signal(true);

  protected readonly authFormReady = computed(() => {
    const emailReady = this.emailValue().includes('@');
    const passwordReady = this.passwordValue().length >= 12;
    return this.mode() === 'login'
      ? emailReady && passwordReady
      : emailReady && passwordReady && this.displayName().trim().length > 0;
  });

  constructor() {
    effect(() => {
      this.memory.saveWelcomeDraft({
        mode: this.mode(), step: this.step(), email: this.emailValue(), displayName: this.displayName(),
        firstName: this.firstName(), lastName: this.lastName(), selectedAccountId: this.selectedAccountId(),
      });
    });

    effect(() => {
      if (this.auth.phase() === 'signedOut') {
        if (!this.draft) this.mode.set(this.auth.hasAccount() ? 'login' : 'create');
        if (this.auth.hasAccount() && !this.emailValue()) {
          const account = this.auth.knownAccounts()[0];
          if (account) this.selectAccount(account);
        }
      }
      const onboarding = this.auth.onboarding();
      if (this.auth.authenticated() && onboarding) {
        if (onboarding.completed) {
          this.memory.clearWelcomeDraft();
          void this.router.navigate(['/']);
          return;
        }
        this.storageDirectory.set(onboarding.storageDirectory ?? this.storageDirectory());
        this.beginnerMode.set(onboarding.beginnerMode);
        this.notifications.set(onboarding.notifications);
        this.confirmDestructive.set(onboarding.confirmDestructiveActions);
        if (this.step() === 0) this.step.set(1);
      }
    });
  }

  protected switchMode(mode: 'create' | 'login'): void {
    this.mode.set(mode);
    this.passwordValue.set('');
    this.passwordVisible.set(false);
    if (mode === 'login' && !this.emailValue()) {
      const account = this.auth.knownAccounts()[0];
      if (account) this.selectAccount(account);
    }
  }

  protected selectAccount(account: AccountProfile): void {
    this.mode.set('login');
    this.selectedAccountId.set(account.id);
    this.emailValue.set(account.email);
    this.passwordValue.set('');
    this.passwordVisible.set(false);
  }

  protected initials(account: AccountProfile): string {
    const values = [account.firstName, account.lastName, account.displayName]
      .filter(Boolean).flatMap((value) => value.trim().split(/\s+/)).filter(Boolean);
    return values.slice(0, 2).map((value) => value[0]?.toUpperCase() ?? '').join('') || 'FF';
  }

  protected async submitAuth(): Promise<void> {
    if (!this.authFormReady() || this.busy()) return;
    this.busy.set(true);
    const ok = this.mode() === 'create'
      ? await this.auth.createAccount({
          email: this.emailValue(), password: this.passwordValue(), displayName: this.displayName(),
          firstName: this.firstName(), lastName: this.lastName(), rememberDevice: this.rememberDevice(),
        })
      : await this.auth.login({
          email: this.emailValue(), password: this.passwordValue(), rememberDevice: this.rememberDevice(),
        });
    this.busy.set(false);
    this.passwordValue.set('');
    this.passwordVisible.set(false);
    if (!ok) return;
    this.memory.clearWelcomeDraft();
    if (this.auth.setupComplete()) {
      await this.router.navigate(['/']);
      return;
    }
    this.step.set(1);
    if (!this.storageDirectory()) await this.useRecommendedFolder();
  }

  protected async chooseFolder(): Promise<void> {
    if (this.folderBusy()) return;
    this.folderBusy.set(true);
    this.folderError.set(null);
    try {
      const selected = await this.auth.chooseStorageDirectory();
      if (selected) this.storageDirectory.set(selected);
    } catch (error) {
      this.folderError.set(this.folderMessage(error));
    } finally {
      this.folderBusy.set(false);
    }
  }

  protected async useRecommendedFolder(): Promise<void> {
    if (this.folderBusy()) return;
    this.folderBusy.set(true);
    this.folderError.set(null);
    try {
      this.storageDirectory.set(await this.auth.defaultStorageDirectory());
    } catch (error) {
      this.folderError.set(this.folderMessage(error));
    } finally {
      this.folderBusy.set(false);
    }
  }

  private folderMessage(error: unknown): string {
    return error instanceof Error ? error.message : String(error || 'Impossible d’ouvrir le sélecteur de dossier.');
  }

  protected async savePreferencesAndContinue(): Promise<void> {
    const ok = await this.auth.saveSetup({
      storageDirectory: this.storageDirectory(), beginnerMode: this.beginnerMode(), preserveOriginals: true,
      notifications: this.notifications(), confirmDestructiveActions: this.confirmDestructive(),
    });
    if (ok) {
      this.applyPreferences();
      this.step.set(3);
    }
  }

  private applyPreferences(): void {
    this.prefs.beginnerMode.set(this.beginnerMode());
    this.prefs.notifyOnCompletion.set(this.notifications());
    this.prefs.confirmDestructive.set(this.confirmDestructive());
    this.prefs.preserveTree.set(true);
    this.prefs.destination.set('subfolder');
  }

  protected async finish(destination = '/'): Promise<void> {
    if (this.busy()) return;
    this.busy.set(true);
    const ok = await this.auth.saveSetup({
      storageDirectory: this.storageDirectory(), beginnerMode: this.beginnerMode(), preserveOriginals: true,
      notifications: this.notifications(), confirmDestructiveActions: this.confirmDestructive(),
    }, true);
    this.busy.set(false);
    if (ok) {
      this.applyPreferences();
      this.memory.clearWelcomeDraft();
      await this.router.navigateByUrl(destination);
    }
  }
}
