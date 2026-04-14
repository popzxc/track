import { createRouter, createWebHistory } from 'vue-router'

import ProjectsPage from '../pages/ProjectsPage.vue'
import ReviewsPage from '../pages/ReviewsPage.vue'
import RunsPage from '../pages/RunsPage.vue'
import SettingsPage from '../pages/SettingsPage.vue'
import TasksPage from '../pages/TasksPage.vue'

export const appRoutes = [
  {
    path: '/',
    redirect: { name: 'tasks' },
  },
  {
    path: '/tasks',
    name: 'tasks',
    component: TasksPage,
  },
  {
    path: '/reviews',
    name: 'reviews',
    component: ReviewsPage,
  },
  {
    path: '/runs',
    name: 'runs',
    component: RunsPage,
  },
  {
    path: '/projects',
    name: 'projects',
    component: ProjectsPage,
  },
  {
    path: '/settings',
    name: 'settings',
    component: SettingsPage,
  },
]

export function createAppRouter() {
  return createRouter({
    history: createWebHistory(),
    routes: appRoutes,
  })
}
