import type { LocationQuery, LocationQueryRaw, RouteLocationNormalizedLoaded, Router } from 'vue-router'

export function firstQueryValue(value: LocationQuery[string]): string | null {
  if (typeof value === 'string') {
    return value
  }

  if (Array.isArray(value)) {
    return value[0] ?? null
  }

  return null
}

export function queryFlag(value: LocationQuery[string]): boolean {
  return firstQueryValue(value) === '1'
}

export async function replaceRouteQuery(
  router: Router,
  route: RouteLocationNormalizedLoaded,
  patch: Record<string, string | null | undefined>,
) {
  const nextQuery: LocationQueryRaw = { ...route.query }

  for (const [key, value] of Object.entries(patch)) {
    if (!value) {
      delete nextQuery[key]
      continue
    }

    nextQuery[key] = value
  }

  await router.replace({
    query: nextQuery,
  })
}
