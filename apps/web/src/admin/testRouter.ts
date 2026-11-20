import { createMemoryHistory, createRouter, type RouteRecordRaw } from 'vue-router'

// Tests only: the given routes plus a catch-all like the real router's, so RouterLinks to other pages resolve.
export const testRouter = (routes: RouteRecordRaw[] = []) =>
  createRouter({ history: createMemoryHistory(), routes: [...routes, { path: '/:path(.*)*', component: { render: () => null } }] })
