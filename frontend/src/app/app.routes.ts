import { requireAccountGuard, welcomeGuard } from './core/auth/auth.guard';
import { Routes } from '@angular/router';

export const routes: Routes = [
  {
    path: 'welcome',
    canActivate: [welcomeGuard],
    loadComponent: () => import('./features/welcome/welcome.page').then((m) => m.WelcomePage),
  },
  {
    path: '',
    canActivate: [requireAccountGuard],
    loadComponent: () => import('./features/home/home.page').then((m) => m.HomePage),
  },
  {
    path: 'conversion',
    canActivate: [requireAccountGuard],
    loadComponent: () => import('./features/workspace/workspace.page').then((m) => m.WorkspacePage),
  },
  {
    path: 'conversion/:actionId',
    canActivate: [requireAccountGuard],
    loadComponent: () => import('./features/workspace/workspace.page').then((m) => m.WorkspacePage),
  },
  { path: 'workspace', redirectTo: 'conversion', pathMatch: 'full' },
  {
    path: 'organize',
    canActivate: [requireAccountGuard],
    loadComponent: () => import('./features/organize/organize.page').then((m) => m.OrganizePage),
  },
  {
    path: 'automations',
    canActivate: [requireAccountGuard],
    loadComponent: () => import('./features/automations/automations.page').then((m) => m.AutomationsPage),
  },
  {
    path: 'history',
    canActivate: [requireAccountGuard],
    loadComponent: () => import('./features/history/history.page').then((m) => m.HistoryPage),
  },
  {
    path: 'favorites',
    canActivate: [requireAccountGuard],
    loadComponent: () => import('./features/favorites/favorites.page').then((m) => m.FavoritesPage),
  },
  {
    path: 'advanced',
    canActivate: [requireAccountGuard],
    loadComponent: () => import('./features/advanced/advanced.page').then((m) => m.AdvancedPage),
  },
  {
    path: 'formats',
    canActivate: [requireAccountGuard],
    loadComponent: () => import('./features/formats/formats.page').then((m) => m.FormatsPage),
  },
  {
    path: 'help',
    canActivate: [requireAccountGuard],
    loadComponent: () => import('./features/help/help.page').then((m) => m.HelpPage),
  },
  {
    path: 'settings',
    redirectTo: 'settings/general',
    pathMatch: 'full',
  },
  {
    path: 'settings/:section',
    canActivate: [requireAccountGuard],
    loadComponent: () => import('./features/settings/settings.page').then((m) => m.SettingsPage),
  },
  { path: '**', redirectTo: '' },
];
