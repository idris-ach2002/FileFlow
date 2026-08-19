import { Routes } from '@angular/router';

export const routes: Routes = [
  {
    path: '',
    loadComponent: () => import('./features/home/home.page').then((m) => m.HomePage),
  },
  {
    path: 'workspace',
    loadComponent: () => import('./features/workspace/workspace.page').then((m) => m.WorkspacePage),
  },
  {
    path: 'automations',
    loadComponent: () => import('./features/automations/automations.page').then((m) => m.AutomationsPage),
  },
  {
    path: 'history',
    loadComponent: () => import('./features/history/history.page').then((m) => m.HistoryPage),
  },
  {
    path: 'settings',
    loadComponent: () => import('./features/settings/settings.page').then((m) => m.SettingsPage),
  },
  { path: '**', redirectTo: '' },
];
