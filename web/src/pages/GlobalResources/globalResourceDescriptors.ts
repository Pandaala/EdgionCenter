import type {
  EdgionConfigDataType,
  GlobalResourceApiSlug,
  GlobalResourceKind,
} from '@/api/globalResources'

export interface GlobalResourceDescriptor {
  key: string
  route: string
  kind: GlobalResourceKind
  apiSlug: GlobalResourceApiSlug
  titleKey?: string
  configDataType?: EdgionConfigDataType
}

const CONFIG_DATA_ROUTE = '/global-resources/edgion-config-data'

export const GLOBAL_RESOURCE_DESCRIPTORS = [
  {
    key: 'http-route',
    route: '/global-resources/http-route',
    kind: 'HTTPRoute',
    apiSlug: 'http-route',
  },
  {
    key: 'grpc-route',
    route: '/global-resources/grpc-route',
    kind: 'GRPCRoute',
    apiSlug: 'grpc-route',
  },
  {
    key: 'edgion-plugins',
    route: '/global-resources/edgion-plugins',
    kind: 'EdgionPlugins',
    apiSlug: 'edgion-plugins',
  },
  {
    key: 'edgion-config-data',
    route: CONFIG_DATA_ROUTE,
    kind: 'EdgionConfigData',
    apiSlug: 'edgion-config-data',
  },
  {
    key: 'reference-grant',
    route: '/global-resources/reference-grant',
    kind: 'ReferenceGrant',
    apiSlug: 'reference-grant',
  },
] as const satisfies readonly GlobalResourceDescriptor[]

export type GlobalResourceDescriptorKey =
  (typeof GLOBAL_RESOURCE_DESCRIPTORS)[number]['key']

export function findGlobalResourceDescriptor(
  route: string,
): GlobalResourceDescriptor | undefined {
  return GLOBAL_RESOURCE_DESCRIPTORS.find((descriptor) => descriptor.route === route)
}
