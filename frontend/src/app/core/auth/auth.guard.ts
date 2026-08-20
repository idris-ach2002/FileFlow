import { inject } from '@angular/core';
import { CanActivateFn, Router } from '@angular/router';
import { AuthStore } from './auth.store';

export const requireAccountGuard: CanActivateFn = async () => {
  const auth = inject(AuthStore);
  const router = inject(Router);
  await auth.initialize();
  return auth.needsWelcome() ? router.createUrlTree(['/welcome']) : true;
};

export const welcomeGuard: CanActivateFn = async () => {
  const auth = inject(AuthStore);
  const router = inject(Router);
  await auth.initialize();
  return auth.setupComplete() ? router.createUrlTree(['/']) : true;
};
