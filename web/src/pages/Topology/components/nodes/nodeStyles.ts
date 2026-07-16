export interface NodeTypeConfig {
  color: string
  bgColor: string
  label: string
}

export const NODE_TYPE_CONFIG: Record<string, NodeTypeConfig> = {
  edgiongatewayconfig: { color: '#531dab', bgColor: '#f9f0ff', label: 'GatewayConfig' },
  gatewayclass:  { color: '#1677ff', bgColor: '#e6f4ff', label: 'GatewayClass' },
  gateway:       { color: '#1890ff', bgColor: '#e6f7ff', label: 'infra.gateway'   },
  httproute:     { color: '#52c41a', bgColor: '#f6ffed', label: 'route.http'      },
  grpcroute:     { color: '#13c2c2', bgColor: '#e6fffb', label: 'route.grpc'      },
  tcproute:      { color: '#2f54eb', bgColor: '#f0f5ff', label: 'route.tcp'       },
  udproute:      { color: '#eb2f96', bgColor: '#fff0f6', label: 'route.udp'       },
  tlsroute:      { color: '#fa8c16', bgColor: '#fff7e6', label: 'route.tls'       },
  service:       { color: '#389e0d', bgColor: '#f6ffed', label: 'infra.service'   },
  endpointslice: { color: '#7cb305', bgColor: '#fcffe6', label: 'EndpointSlice' },
  backend:       { color: '#5cdbd3', bgColor: '#e6fffb', label: 'Backend' },
  backendtlspolicy: { color: '#d4380d', bgColor: '#fff2e8', label: 'BackendTLSPolicy' },
  edgionbackendtrafficpolicy: { color: '#d4b106', bgColor: '#feffe6', label: 'BackendTrafficPolicy' },
  edgionplugins: { color: '#fa541c', bgColor: '#fff2e8', label: 'plugins.edgion'  },
  edgionstreamplugins: { color: '#fa8c16', bgColor: '#fff7e6', label: 'StreamPlugins' },
  edgionconfigdata: { color: '#722ed1', bgColor: '#f9f0ff', label: 'ConfigData' },
  linksys:       { color: '#2f54eb', bgColor: '#f0f5ff', label: 'LinkSys' },
  edgionacme:    { color: '#08979c', bgColor: '#e6fffb', label: 'ACME' },
  edgiontls:     { color: '#eb2f96', bgColor: '#fff0f6', label: 'security.tls'    },
  secret:        { color: '#8c8c8c', bgColor: '#fafafa', label: 'security.secret' },
  configmap:     { color: '#595959', bgColor: '#fafafa', label: 'ConfigMap' },
  referencegrant:{ color: '#ad6800', bgColor: '#fffbe6', label: 'ReferenceGrant' },
  unknown:       { color: '#cf1322', bgColor: '#fff1f0', label: 'Unknown reference' },
}
