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
  configDataType: EdgionConfigDataType
}

const CONFIG_DATA_ROUTE = '/global-resources/edgion-config-data'

export const GLOBAL_RESOURCE_DESCRIPTORS = [
  {
    key: 'ip-list',
    route: `${CONFIG_DATA_ROUTE}/ip-list`,
    kind: 'EdgionConfigData',
    apiSlug: 'edgion-config-data',
    configDataType: 'IpList',
  },
  {
    key: 'key-list',
    route: `${CONFIG_DATA_ROUTE}/key-list`,
    kind: 'EdgionConfigData',
    apiSlug: 'edgion-config-data',
    configDataType: 'KeyList',
  },
  {
    key: 'selector',
    route: `${CONFIG_DATA_ROUTE}/selector`,
    kind: 'EdgionConfigData',
    apiSlug: 'edgion-config-data',
    configDataType: 'Selector',
  },
  {
    key: 'misc',
    route: `${CONFIG_DATA_ROUTE}/misc`,
    kind: 'EdgionConfigData',
    apiSlug: 'edgion-config-data',
    configDataType: 'Misc',
  },
] as const satisfies readonly GlobalResourceDescriptor[]

export type GlobalResourceDescriptorKey =
  (typeof GLOBAL_RESOURCE_DESCRIPTORS)[number]['key']

export function findGlobalResourceDescriptor(
  route: string,
): GlobalResourceDescriptor | undefined {
  return GLOBAL_RESOURCE_DESCRIPTORS.find((descriptor) => descriptor.route === route)
}
