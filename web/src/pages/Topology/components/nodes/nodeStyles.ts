export interface NodeTypeConfig {
  color: string
  bgColor: string
  label: string
}

export const NODE_TYPE_CONFIG: Record<string, NodeTypeConfig> = {
  gateway:       { color: '#1890ff', bgColor: '#e6f7ff', label: 'infra.gateway'   },
  httproute:     { color: '#52c41a', bgColor: '#f6ffed', label: 'route.http'      },
  grpcroute:     { color: '#13c2c2', bgColor: '#e6fffb', label: 'route.grpc'      },
  tcproute:      { color: '#2f54eb', bgColor: '#f0f5ff', label: 'route.tcp'       },
  udproute:      { color: '#eb2f96', bgColor: '#fff0f6', label: 'route.udp'       },
  tlsroute:      { color: '#fa8c16', bgColor: '#fff7e6', label: 'route.tls'       },
  service:       { color: '#389e0d', bgColor: '#f6ffed', label: 'infra.service'   },
  edgionplugins: { color: '#fa541c', bgColor: '#fff2e8', label: 'plugins.edgion'  },
  edgiontls:     { color: '#eb2f96', bgColor: '#fff0f6', label: 'security.tls'    },
  secret:        { color: '#8c8c8c', bgColor: '#fafafa', label: 'security.secret' },
}
