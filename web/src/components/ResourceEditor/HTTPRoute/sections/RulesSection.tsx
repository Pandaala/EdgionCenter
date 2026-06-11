/**
 * Rules Section - 路由规则配置
 * 最复杂的部分，包含 matches, filters, backendRefs
 */

import React from 'react';
import { Card, Collapse, Button, Tag, Typography, Space } from 'antd';
import { PlusOutlined } from '@ant-design/icons';
import MatchesEditor from '../rule-components/MatchesEditor';
import BackendRefsEditor from '../rule-components/BackendRefsEditor';
import { DEFAULT_VALUES } from '@/constants/gateway-api';
import type { HTTPRouteRule } from '@/types/gateway-api';

const { Panel } = Collapse;
const { Text } = Typography;

interface RulesSectionProps {
  value?: HTTPRouteRule[];
  onChange?: (value: HTTPRouteRule[]) => void;
  disabled?: boolean;
  namespace?: string;
}

const RulesSection: React.FC<RulesSectionProps> = ({
  value = [],
  onChange,
  disabled = false,
  namespace = DEFAULT_VALUES.defaultNamespace,
}) => {
  const handleRuleChange = (index: number, updatedRule: HTTPRouteRule) => {
    const newRules = [...value];
    newRules[index] = updatedRule;
    onChange?.(newRules);
  };

  const handleAddRule = () => {
    onChange?.([
      ...value,
      {
        matches: [
          {
            path: {
              type: DEFAULT_VALUES.pathMatch.type,
              value: DEFAULT_VALUES.pathMatch.value,
            },
          },
        ],
        backendRefs: [
          {
            group: DEFAULT_VALUES.backendRef.group,
            kind: DEFAULT_VALUES.backendRef.kind,
            namespace: namespace,
            name: '',
            port: 80,
            weight: DEFAULT_VALUES.backendRef.weight,
          },
        ],
      },
    ]);
  };

  const handleRemoveRule = (index: number) => {
    const newRules = value.filter((_, i) => i !== index);
    onChange?.(newRules);
  };

  /**
   * 获取规则摘要（显示在 Panel Header）
   */
  const getRuleSummary = (rule: HTTPRouteRule, index: number) => {
    const pathMatch = rule.matches?.[0]?.path;
    const backendCount = rule.backendRefs?.length || 0;
    const filterCount = rule.filters?.length || 0;

    return (
      <Space>
        <Text strong>规则 {index + 1}</Text>
        {pathMatch && (
          <>
            <Tag color="blue">{pathMatch.type || 'PathPrefix'}</Tag>
            <Text code>{pathMatch.value || '/'}</Text>
          </>
        )}
        <Text type="secondary">
          → {backendCount} 个后端服务
          {filterCount > 0 && `, ${filterCount} 个过滤器`}
        </Text>
      </Space>
    );
  };

  return (
    <Card title="路由规则 / Rules" size="small">
      {value.length > 0 ? (
        <Collapse accordion>
          {value.map((rule, index) => (
            <Panel
              header={getRuleSummary(rule, index)}
              key={index}
              extra={
                !disabled && (
                  <Button
                    danger
                    size="small"
                    onClick={(e) => {
                      e.stopPropagation();
                      handleRemoveRule(index);
                    }}
                  >
                    删除规则
                  </Button>
                )
              }
            >
              {/* 匹配条件 */}
              <Card
                type="inner"
                size="small"
                title="匹配条件 / Matches"
                style={{ marginBottom: 16 }}
              >
                <MatchesEditor
                  value={rule.matches}
                  onChange={(matches) => handleRuleChange(index, { ...rule, matches })}
                  disabled={disabled}
                />
              </Card>

              {/* 后端服务 */}
              <Card
                type="inner"
                size="small"
                title="后端服务 / BackendRefs"
                style={{ marginBottom: 16 }}
              >
                <BackendRefsEditor
                  value={rule.backendRefs}
                  onChange={(backendRefs) => handleRuleChange(index, { ...rule, backendRefs })}
                  disabled={disabled}
                  namespace={namespace}
                />
              </Card>

              {/* 过滤器（TODO：待实现） */}
              <Card
                type="inner"
                size="small"
                title="过滤器 / Filters（TODO）"
              >
                <Text type="secondary">
                  过滤器功能（如 Header 修改、重定向、URL 重写等）将在后续版本实现
                </Text>
              </Card>
            </Panel>
          ))}
        </Collapse>
      ) : (
        <Text type="secondary">暂无路由规则</Text>
      )}

      {!disabled && (
        <Button
          type="dashed"
          onClick={handleAddRule}
          block
          icon={<PlusOutlined />}
          style={{ marginTop: 16 }}
        >
          添加路由规则
        </Button>
      )}
    </Card>
  );
};

export default RulesSection;

