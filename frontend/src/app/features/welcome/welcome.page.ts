import { ChangeDetectionStrategy, Component, computed, effect, inject, signal } from '@angular/core';
import { Router } from '@angular/router';
import { AuthStore } from '../../core/auth/auth.store';
import { PreferencesService } from '../../core/preferences/preferences.service';

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
          <div class="error-banner" role="alert"><strong>Impossible de continuer</strong><span>{{ error }}</span><button type="button" (click)="auth.clearError()">Fermer</button></div>
        }

        @switch (step()) {
          @case (0) {
            <section class="step-panel auth-panel">
              <p class="eyebrow">BIENVENUE</p>
              <h1>{{ mode() === 'create' ? 'Créons votre espace FileFlow.' : 'Content de vous revoir.' }}</h1>
              <p class="lead">{{ mode() === 'create' ? 'Votre profil, vos préférences et votre historique restent associés à ce profil local. Vos documents ne sont pas envoyés vers un compte cloud.' : 'Connectez-vous à votre profil sur cet appareil.' }}</p>

              <div class="mode-switch">
                <button type="button" [class.active]="mode() === 'create'" (click)="mode.set('create')">Créer un compte</button>
                <button type="button" [class.active]="mode() === 'login'" (click)="mode.set('login')">Se connecter</button>
              </div>

              @if (mode() === 'create') {
                <div class="form-grid two">
                  <label><span>Prénom</span><input #first autocomplete="given-name" [value]="firstName()" (input)="firstName.set(first.value)" placeholder="Idris" /></label>
                  <label><span>Nom</span><input #last autocomplete="family-name" [value]="lastName()" (input)="lastName.set(last.value)" placeholder="Achabou" /></label>
                </div>
                <label><span>Nom affiché</span><input #display autocomplete="nickname" [value]="displayName()" (input)="displayName.set(display.value)" placeholder="Comment FileFlow doit vous appeler" /></label>
              }
              <label><span>Adresse e-mail</span><input #email type="email" autocomplete="email" [value]="emailValue()" (input)="emailValue.set(email.value)" placeholder="vous@exemple.fr" /></label>
              <label><span>Mot de passe</span><input #password type="password" [attr.autocomplete]="mode() === 'create' ? 'new-password' : 'current-password'" [value]="passwordValue()" (input)="passwordValue.set(password.value)" placeholder="Au moins 12 caractères" /></label>

              <button class="primary-action" type="button" [disabled]="busy() || !authFormReady()" (click)="submitAuth()">
                {{ busy() ? 'Sécurisation en cours…' : (mode() === 'create' ? 'Créer mon espace' : 'Me connecter') }}
              </button>
              <div class="security-note"><span>◆</span><p><strong>Mot de passe protégé.</strong> FileFlow enregistre uniquement une dérivation lente avec sel unique. Le mot de passe n’est jamais stocké en clair.</p></div>
            </section>
          }

          @case (1) {
            <section class="step-panel">
              <p class="eyebrow">ÉTAPE 2 · VOS RÉSULTATS</p>
              <h1>Où voulez-vous retrouver vos fichiers ?</h1>
              <p class="lead">FileFlow conservera toujours vos originaux. Ce dossier sert de destination claire pour les nouveaux résultats.</p>

              <button class="folder-choice" type="button" [disabled]="folderBusy()" (click)="chooseFolder()">
                <span class="folder-icon">▰</span>
                <span><strong>{{ storageDirectory() ? 'Dossier FileFlow' : 'Choisir un dossier' }}</strong><small>{{ storageDirectory() || 'Sélectionnez l’emplacement qui vous convient.' }}</small></span>
                <b>{{ folderBusy() ? 'Ouverture…' : 'Modifier' }}</b>
              </button>
              <button class="secondary-action" type="button" [disabled]="folderBusy()" (click)="useRecommendedFolder()">Utiliser le dossier recommandé</button>
              @if (folderError()) { <p class="folder-error">{{ folderError() }}</p> }

              <div class="simple-rule"><span>✓</span><div><strong>Originaux intacts</strong><p>Une conversion produit un nouveau fichier. FileFlow ne remplace pas vos documents silencieusement.</p></div></div>

              <div class="footer-actions"><button type="button" class="ghost" (click)="step.set(0)">Retour</button><button type="button" class="primary-action inline" [disabled]="!storageDirectory() || folderBusy()" (click)="step.set(2)">Continuer</button></div>
            </section>
          }

          @case (2) {
            <section class="step-panel">
              <p class="eyebrow">ÉTAPE 3 · VOTRE CONFORT</p>
              <h1>FileFlow peut faire simple.</h1>
              <p class="lead">Ces choix peuvent être modifiés plus tard dans Paramètres. Nous avons sélectionné les options les plus sûres.</p>

              <label class="choice-row"><input type="checkbox" [checked]="beginnerMode()" (change)="beginnerMode.set(!beginnerMode())" /><span><strong>Me guider étape par étape</strong><small>FileFlow explique quoi faire et masque les détails inutiles.</small></span></label>
              <label class="choice-row"><input type="checkbox" [checked]="preserveOriginals()" disabled /><span><strong>Toujours conserver mes originaux</strong><small>Protection activée par défaut. Les actions destructrices nécessitent une confirmation claire.</small></span></label>
              <label class="choice-row"><input type="checkbox" [checked]="notifications()" (change)="notifications.set(!notifications())" /><span><strong>Me prévenir quand un long traitement est terminé</strong><small>Utile pour les vidéos, OCR et gros dossiers.</small></span></label>
              <label class="choice-row"><input type="checkbox" [checked]="confirmDestructive()" (change)="confirmDestructive.set(!confirmDestructive())" /><span><strong>Demander avant les actions sensibles</strong><small>Suppression de métadonnées, renommage et organisation en masse.</small></span></label>

              <div class="footer-actions"><button type="button" class="ghost" (click)="step.set(1)">Retour</button><button type="button" class="primary-action inline" (click)="savePreferencesAndContinue()">Continuer</button></div>
            </section>
          }

          @case (3) {
            <section class="step-panel final-panel">
              <div class="success-orb">✓</div>
              <p class="eyebrow">PRÊT</p>
              <h1>Vous n’avez rien à apprendre.</h1>
              <p class="lead">Dites simplement ce que vous voulez faire. FileFlow vous proposera les bons choix au bon moment.</p>

              <div class="tour-grid">
                <article><span>1</span><strong>Choisissez votre intention</strong><p>« Réduire un PDF », « convertir des photos », « ouvrir une archive »…</p></article>
                <article><span>2</span><strong>Ajoutez vos fichiers</strong><p>Cliquez ou glissez-les depuis Finder. FileFlow reconnaît leur vrai format.</p></article>
                <article><span>3</span><strong>Validez</strong><p>Le résultat est créé à côté ou dans votre dossier FileFlow, sans toucher à l’original.</p></article>
              </div>

              <button class="primary-action" type="button" [disabled]="busy()" (click)="finish()">{{ busy() ? 'Préparation…' : 'Ouvrir FileFlow' }}</button>
              <button class="help-link" type="button" (click)="finish('/help')">Voir d’abord le guide d’utilisation</button>
            </section>
          }
        }
      </section>
      <aside class="welcome-aside">
        <div class="promise"><span>01</span><strong>Simple par défaut</strong><p>Les réglages techniques restent cachés tant que vous n’en avez pas besoin.</p></div>
        <div class="promise"><span>02</span><strong>Local par défaut</strong><p>Les transformations de fichiers sont exécutées sur votre machine.</p></div>
        <div class="promise"><span>03</span><strong>Guidé si besoin</strong><p>Un centre d’aide explique chaque action avec des mots simples et des exemples.</p></div>
      </aside>
    </main>
  `,
  styles: [`
    :host{display:block;min-height:100vh;background:radial-gradient(circle at 18% 10%,color-mix(in srgb,var(--accent) 10%,transparent),transparent 34%),var(--bg);color:var(--text)}
    .welcome-shell{min-height:100vh;display:grid;grid-template-columns:minmax(0,720px) minmax(280px,380px);justify-content:center;align-items:center;gap:34px;padding:36px}
    .welcome-card{min-height:690px;padding:30px 42px 36px;border:1px solid var(--border);border-radius:28px;background:color-mix(in srgb,var(--surface-1) 96%,transparent);box-shadow:var(--shadow-lg)}
    .welcome-brand{display:flex;align-items:center;gap:12px}.logo{width:42px;height:42px;display:grid;place-items:center;border-radius:14px;background:linear-gradient(145deg,var(--accent),#7659e9);color:white;font-weight:900;box-shadow:0 10px 26px color-mix(in srgb,var(--accent) 25%,transparent)}.welcome-brand strong,.welcome-brand span{display:block}.welcome-brand strong{font-size:17px}.welcome-brand span{margin-top:2px;color:var(--text-muted);font-size:12px}
    .progress{display:grid;grid-template-columns:repeat(4,1fr);gap:7px;margin:28px 0 36px}.progress span{height:4px;border-radius:999px;background:var(--surface-3)}.progress span.active{background:var(--accent)}
    .step-panel{max-width:570px}.eyebrow{margin:0 0 9px;color:var(--accent);font-size:11px;font-weight:900;letter-spacing:.13em}.step-panel h1{margin:0;font-size:clamp(34px,5vw,52px);line-height:1.02;letter-spacing:-.055em}.lead{margin:14px 0 26px;color:var(--text-muted);font-size:15px;line-height:1.65}
    .mode-switch{display:grid;grid-template-columns:1fr 1fr;padding:4px;margin-bottom:18px;border-radius:13px;background:var(--surface-2)}.mode-switch button{min-height:40px;border:0;border-radius:10px;background:transparent;color:var(--text-muted);font-weight:750}.mode-switch button.active{background:var(--surface-1);color:var(--text);box-shadow:var(--shadow-sm)}
    label{display:block;margin:12px 0}label>span{display:block;margin-bottom:6px;color:var(--text-muted);font-size:12px;font-weight:750}input:not([type=checkbox]){box-sizing:border-box;width:100%;height:47px;padding:0 13px;border:1px solid var(--border);border-radius:12px;outline:none;background:var(--surface-2);color:var(--text);font:inherit}input:focus{border-color:var(--accent);box-shadow:0 0 0 3px var(--accent-soft)}.form-grid.two{display:grid;grid-template-columns:1fr 1fr;gap:12px}.form-grid.two label{margin-top:0}
    .primary-action,.secondary-action,.ghost,.help-link{border:0;font:inherit;cursor:pointer}.primary-action{width:100%;min-height:50px;margin-top:18px;border-radius:13px;background:var(--accent);color:white;font-weight:850;box-shadow:0 10px 24px color-mix(in srgb,var(--accent) 22%,transparent)}.primary-action:disabled{opacity:.45;cursor:not-allowed}.primary-action.inline{width:auto;min-width:150px;margin:0}.secondary-action{min-height:40px;padding:0 14px;border-radius:10px;background:var(--surface-2);color:var(--text-muted);font-weight:750}.ghost{min-height:40px;padding:0 14px;background:transparent;color:var(--text-muted);font-weight:750}.help-link{display:block;margin:14px auto 0;background:transparent;color:var(--accent);font-size:12px;font-weight:800}
    .security-note,.simple-rule{display:flex;gap:10px;margin-top:17px;padding:12px 13px;border-radius:12px;background:var(--bg-elevated);color:var(--text-muted)}.security-note>span,.simple-rule>span{color:var(--success);font-weight:900}.security-note p,.simple-rule p{margin:0;font-size:11px;line-height:1.5}.security-note strong,.simple-rule strong{color:var(--text)}
    .folder-choice{width:100%;display:grid;grid-template-columns:44px minmax(0,1fr) auto;align-items:center;gap:12px;padding:14px;border:1px solid var(--border);border-radius:15px;background:var(--surface-2);color:var(--text);text-align:left}.folder-icon{width:42px;height:42px;display:grid;place-items:center;border-radius:12px;background:var(--accent-soft);color:var(--accent)}.folder-choice strong,.folder-choice small{display:block}.folder-choice small{overflow:hidden;margin-top:4px;color:var(--text-muted);font-size:11px;text-overflow:ellipsis;white-space:nowrap}.folder-choice b{color:var(--accent);font-size:11px}.secondary-action{margin-top:10px}.folder-error{margin:10px 0 0;color:var(--danger,#d14b4b);font-size:12px;line-height:1.45}.simple-rule{margin:24px 0}.footer-actions{display:flex;align-items:center;justify-content:space-between;margin-top:28px}
    .choice-row{display:grid;grid-template-columns:28px minmax(0,1fr);gap:10px;align-items:start;margin:9px 0;padding:13px;border:1px solid var(--border);border-radius:13px;background:var(--surface-2);cursor:pointer}.choice-row input{width:18px;height:18px;margin:2px 0 0;accent-color:var(--accent)}.choice-row span{margin:0}.choice-row strong,.choice-row small{display:block}.choice-row strong{color:var(--text);font-size:13px}.choice-row small{margin-top:4px;color:var(--text-muted);font-size:11px;line-height:1.45}
    .success-orb{width:62px;height:62px;display:grid;place-items:center;margin-bottom:20px;border-radius:20px;background:color-mix(in srgb,var(--success) 15%,var(--surface-1));color:var(--success);font-size:28px;font-weight:900}.tour-grid{display:grid;grid-template-columns:repeat(3,1fr);gap:10px;margin:24px 0}.tour-grid article{padding:14px;border:1px solid var(--border);border-radius:13px;background:var(--surface-2)}.tour-grid article>span{width:25px;height:25px;display:grid;place-items:center;margin-bottom:10px;border-radius:8px;background:var(--accent-soft);color:var(--accent);font-size:10px;font-weight:900}.tour-grid strong{display:block;font-size:12px}.tour-grid p{margin:6px 0 0;color:var(--text-muted);font-size:10px;line-height:1.5}
    .error-banner{position:relative;margin:-17px 0 20px;padding:12px 42px 12px 13px;border:1px solid color-mix(in srgb,var(--danger) 30%,var(--border));border-radius:12px;background:color-mix(in srgb,var(--danger) 8%,var(--surface-1))}.error-banner strong,.error-banner span{display:block}.error-banner strong{font-size:12px}.error-banner span{margin-top:3px;color:var(--text-muted);font-size:11px}.error-banner button{position:absolute;right:8px;top:8px;border:0;background:transparent;color:var(--text-muted)}
    .welcome-aside{display:grid;gap:16px}.promise{padding:20px 6px;border-top:1px solid var(--border)}.promise>span{color:var(--accent);font-size:10px;font-weight:900}.promise strong{display:block;margin-top:8px;font-size:16px}.promise p{margin:6px 0 0;color:var(--text-muted);font-size:12px;line-height:1.6}
    @media(max-width:930px){.welcome-shell{grid-template-columns:minmax(0,700px);padding:20px}.welcome-aside{display:none}.welcome-card{min-height:auto}}
    @media(max-width:600px){.welcome-shell{padding:0}.welcome-card{min-height:100vh;padding:24px 20px;border:0;border-radius:0}.form-grid.two,.tour-grid{grid-template-columns:1fr}.step-panel h1{font-size:38px}}
  `],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class WelcomePage {
  protected readonly auth = inject(AuthStore);
  private readonly prefs = inject(PreferencesService);
  private readonly router = inject(Router);
  protected readonly step = signal(0);
  protected readonly mode = signal<'create' | 'login'>('create');
  protected readonly busy = signal(false);
  protected readonly folderBusy = signal(false);
  protected readonly folderError = signal<string | null>(null);
  protected readonly firstName = signal('');
  protected readonly lastName = signal('');
  protected readonly displayName = signal('');
  protected readonly emailValue = signal('');
  protected readonly passwordValue = signal('');
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
      if (this.auth.phase() === 'signedOut') {
        this.mode.set(this.auth.hasAccount() ? 'login' : 'create');
        this.step.set(0);
      }
      const onboarding = this.auth.onboarding();
      if (this.auth.authenticated() && onboarding) {
        if (onboarding.completed) {
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

  protected async submitAuth(): Promise<void> {
    if (!this.authFormReady() || this.busy()) return;
    this.busy.set(true);
    const ok = this.mode() === 'create'
      ? await this.auth.createAccount({
          email: this.emailValue(), password: this.passwordValue(), displayName: this.displayName(),
          firstName: this.firstName(), lastName: this.lastName(),
        })
      : await this.auth.login({ email: this.emailValue(), password: this.passwordValue() });
    this.busy.set(false);
    this.passwordValue.set('');
    if (!ok) return;
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

      if (selected) {
        this.storageDirectory.set(selected);
      }
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
      this.storageDirectory.set(
        await this.auth.defaultStorageDirectory()
      );
    } catch (error) {
      this.folderError.set(this.folderMessage(error));
    } finally {
      this.folderBusy.set(false);
    }
  }

  private folderMessage(error: unknown): string {
    return error instanceof Error
      ? error.message
      : String(
          error ||
          'Impossible d’ouvrir le sélecteur de dossier.'
        );
  }

  protected async savePreferencesAndContinue(): Promise<void> {
    const ok = await this.auth.saveSetup({
      storageDirectory: this.storageDirectory(), beginnerMode: this.beginnerMode(),
      preserveOriginals: true, notifications: this.notifications(),
      confirmDestructiveActions: this.confirmDestructive(),
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
    // Guided onboarding always starts from the safest non-destructive defaults.
    this.prefs.preserveTree.set(true);
    this.prefs.destination.set('subfolder');
  }

  protected async finish(destination = '/'): Promise<void> {
    if (this.busy()) return;
    this.busy.set(true);
    const ok = await this.auth.saveSetup({
      storageDirectory: this.storageDirectory(), beginnerMode: this.beginnerMode(),
      preserveOriginals: true, notifications: this.notifications(),
      confirmDestructiveActions: this.confirmDestructive(),
    }, true);
    this.busy.set(false);
    if (ok) {
      this.applyPreferences();
      await this.router.navigateByUrl(destination);
    }
  }
}
