import { theme as antdTheme } from 'antd'
import type { ThemeConfig } from 'antd'
import type { ResolvedMode, ThemeTokens } from './tokens'

export const buildAntdTheme = (
  mode: ResolvedMode,
  t: ThemeTokens,
): ThemeConfig => ({
  algorithm:
    mode === 'dark' ? antdTheme.darkAlgorithm : antdTheme.defaultAlgorithm,
  token: {
    colorPrimary: t.color.brand,
    colorInfo: t.color.info,
    colorSuccess: t.color.success,
    colorWarning: t.color.warning,
    colorError: t.color.danger,
    colorBgBase: t.color.bgBase,
    colorBgContainer: t.color.bgSurface,
    colorBgLayout: t.color.bgBase,
    colorBgElevated: t.color.bgSurface,
    colorBorder: t.color.border,
    colorBorderSecondary: t.color.border,
    colorText: t.color.text,
    colorTextSecondary: t.color.textMuted,
    colorTextTertiary: t.color.textSubtle,
    colorTextPlaceholder: t.color.textSubtle,
    fontFamily: t.font.sans,
    fontFamilyCode: t.font.mono,
    fontSize: t.size.base,
    borderRadius: t.radius.sm,
    borderRadiusLG: t.radius.md,
    controlHeight: 34,
    boxShadow: t.shadow.md,
    boxShadowSecondary: t.shadow.sm,
  },
  components: {
    Button: { borderRadius: t.radius.sm, controlHeight: 34, primaryShadow: 'none' },
    Table: {
      headerBg: t.color.bgSubtle,
      headerColor: t.color.textMuted,
      headerSplitColor: 'transparent',
      cellPaddingBlock: 12,
      cellPaddingInline: 16,
      borderColor: t.color.border,
      rowHoverBg: t.color.bgHover,
    },
    Card: {
      borderRadiusLG: t.radius.md,
      paddingLG: 20,
      colorBorderSecondary: t.color.border,
    },
    Tabs: {
      inkBarColor: t.color.brand,
      itemActiveColor: t.color.brand,
      itemHoverColor: t.color.brand,
      itemSelectedColor: t.color.brand,
      horizontalMargin: '0 24px 0 0',
    },
    Input: { controlHeight: 34, borderRadius: t.radius.sm },
    InputNumber: { controlHeight: 34, borderRadius: t.radius.sm },
    Select: { controlHeight: 34, borderRadius: t.radius.sm },
    Modal: { borderRadiusLG: t.radius.md, paddingLG: 24 },
    Tag: { defaultBg: t.color.bgSubtle, defaultColor: t.color.textMuted },
    Form: { itemMarginBottom: 20, verticalLabelPadding: '0 0 6px' },
  },
})
