import { Result } from 'antd'
import PageHeader from '@/components/PageHeader'
import { useT } from '@/i18n'

interface GlobalRulePendingPageProps {
  resource: 'sharedPlugins' | 'wafControl'
}

export default function GlobalRulePendingPage({ resource }: GlobalRulePendingPageProps) {
  const t = useT()
  const titleKey = resource === 'sharedPlugins'
    ? 'center.nav.globalSharedPlugins'
    : 'center.nav.globalWafControl'
  const descriptionKey = resource === 'sharedPlugins'
    ? 'globalRules.sharedPlugins.pending'
    : 'globalRules.wafControl.pending'

  return (
    <div>
      <PageHeader title={t(titleKey)} subtitle={t('globalRules.subtitle')} />
      <Result
        status="info"
        title={t('globalRules.pending.title')}
        subTitle={t(descriptionKey)}
      />
    </div>
  )
}
