import { useQueries } from '@tanstack/react-query'
import { centerApi, type ControllerSummary } from '@/api/center'
import { listFirstClassResources } from '@/config/resourceCatalog'
import type { ControllerResourceSnapshot } from '@/utils/controller-observability'
import { controllerAccessApi, controllerKindFor, operationIsAllowed } from '@/api/access'

const ENTRIES = listFirstClassResources()

export async function loadControllerObservability(controller: ControllerSummary): Promise<ControllerResourceSnapshot> {
  const access = await controllerAccessApi.get(controller.controller_id).catch(() => null)
  const readableEntries = access ? ENTRIES.filter((entry) => access.resources.find((row) => row.kind === controllerKindFor(entry.kind))?.verbs.includes('list')) : []
  const [settled, diagnostics] = await Promise.all([
    Promise.allSettled(readableEntries.map(async (entry) => ({
    kind: entry.kind,
    resources: (await centerApi.listControllerResources(controller.controller_id, entry.kind, entry.scope)).data ?? [],
    }))),
    access && operationIsAllowed(access, 'diagnostics') ? centerApi.controllerConfConflicts(controller.controller_id).then(
      (value) => ({ available: true, conflicts: value.conflicts ?? [] }),
      () => ({ available: false, conflicts: [] }),
    ) : Promise.resolve({ available: false, conflicts: [] }),
  ])
  const resources: ControllerResourceSnapshot['resources'] = {}
  const errors: ControllerResourceSnapshot['errors'] = ENTRIES.filter((entry) => !readableEntries.includes(entry)).map((entry) => entry.kind)
  settled.forEach((result, index) => {
    if (result.status === 'fulfilled') resources[result.value.kind] = result.value.resources
    else errors.push(readableEntries[index].kind)
  })
  return { controllerId: controller.controller_id, cluster: controller.cluster, resources, errors, fileConflicts: diagnostics.conflicts, diagnosticsAvailable: diagnostics.available }
}

export function useControllerObservability(controllers: ControllerSummary[]) {
  return useQueries({
    queries: controllers.map((controller) => ({
      queryKey: ['controller-observability', controller.controller_id],
      queryFn: () => loadControllerObservability(controller),
      enabled: controller.online,
      staleTime: 30_000,
      retry: 0,
    })),
    combine: (results) => ({
      snapshots: results.flatMap((result) => result.data ? [result.data] : []),
      isLoading: results.some((result) => result.isLoading),
      isFetching: results.some((result) => result.isFetching),
      refetch: () => results.forEach((result) => result.refetch()),
    }),
  })
}
