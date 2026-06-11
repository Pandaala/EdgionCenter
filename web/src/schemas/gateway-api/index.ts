/**
 * Gateway API Schema 统一导出
 */

// 通用 Schema
export {
  dns1123SubdomainSchema,
  dns1123LabelSchema,
  hostnameSchema,
  httpHeaderNameSchema,
  portSchema,
  weightSchema,
  k8sObjectMetaSchema,
  parentReferenceSchema,
  backendRefSchema,
  backendObjectReferenceSchema,
  localObjectReferenceSchema,
  httpHeaderSchema,
} from './common';

// HTTPRoute Schema
export {
  httpPathMatchSchema,
  httpHeaderMatchSchema,
  httpQueryParamMatchSchema,
  httpRouteMatchSchema,
  httpRequestHeaderFilterSchema,
  httpPathModifierSchema,
  httpRequestRedirectFilterSchema,
  httpURLRewriteFilterSchema,
  httpRequestMirrorFilterSchema,
  httpRouteFilterSchema,
  httpRouteTimeoutsSchema,
  httpRouteRuleSchema,
  httpRouteSpecSchema,
  httpRouteSchema,
  type HTTPRouteSchemaType,
} from './httproute';

