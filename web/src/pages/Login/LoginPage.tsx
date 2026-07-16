import { useState, useEffect } from 'react'
import { useLocation, useNavigate } from 'react-router-dom'
import { Form, Input, Button, Alert, Spin, message } from 'antd'
import { UserOutlined, LockOutlined } from '@ant-design/icons'
import { authApi } from '../../api/auth'
import { setLoggedIn, isLoggedIn, takeLoginReturnPath } from '../../utils/auth'
import { getAppMode } from '../../utils/proxy'
import { useT } from '../../i18n/index.tsx'

const LoginPage = ({ passwordLogin = true }: { passwordLogin?: boolean }) => {
  const navigate = useNavigate()
  const location = useLocation()
  const t = useT()
  const [loading, setLoading] = useState(false)
  const [authError, setAuthError] = useState(false)
  const [externalChecking, setExternalChecking] = useState(!passwordLogin)
  const [savedRequestedPath] = useState(() => takeLoginReturnPath())
  const isCenter = getAppMode() === 'center'
  const statePath = (location.state as { from?: unknown } | null)?.from
  const requestedPath = typeof statePath === 'string' && statePath.startsWith('/') && !statePath.startsWith('//')
    ? statePath
    : savedRequestedPath ?? '/'

  useEffect(() => {
    if (isLoggedIn()) {
      navigate(requestedPath, { replace: true })
      return
    }
    if (!passwordLogin) {
      authApi
        .me()
        .then((response) => {
          if (response.success) {
            setLoggedIn()
            navigate(requestedPath, { replace: true })
          }
        })
        .catch(() => undefined)
        .finally(() => setExternalChecking(false))
    }
  }, [navigate, passwordLogin, requestedPath])

  const handleSubmit = async (values: { username: string; password: string }) => {
    setLoading(true)
    setAuthError(false)
    try {
      const res = await authApi.login({ username: values.username, password: values.password })
      if (res.success && res.data) {
        setLoggedIn()
        navigate(requestedPath, { replace: true })
      } else {
        setAuthError(true)
        message.error(t('login.failed'))
      }
    } catch {
      setAuthError(true)
      message.error(t('login.failed'))
    } finally {
      setLoading(false)
    }
  }

  return (
    <div
      style={{
        minHeight: '100vh',
        background: 'var(--ec-color-bg-base)',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        padding: 24,
      }}
    >
      <div
        style={{
          width: 360,
          padding: 32,
          background: 'var(--ec-color-bg-surface)',
          border: '1px solid var(--ec-color-border)',
          borderRadius: 'var(--ec-radius-md)',
          boxShadow: 'var(--ec-shadow-md)',
        }}
      >
        <div
          style={{
            fontSize: 24,
            fontWeight: 600,
            color: 'var(--ec-color-text)',
            marginBottom: 4,
            letterSpacing: '-0.01em',
          }}
        >
          {isCenter ? 'Edgion Center' : 'Edgion'}
        </div>
        <div
          style={{
            fontSize: 'var(--ec-size-sm)',
            color: 'var(--ec-color-text-muted)',
            marginBottom: 24,
          }}
        >
          {t(isCenter ? 'login.subtitle.center' : 'login.subtitle')}
        </div>
        {!passwordLogin ? (
          externalChecking ? (
            <div style={{ display: 'flex', justifyContent: 'center', padding: 24 }}>
              <Spin />
            </div>
          ) : (
            <Alert
              type="info"
              showIcon
              message={t('login.external.title')}
              description={t('login.external.description')}
              action={<Button onClick={() => window.location.reload()}>{t('btn.refresh')}</Button>}
            />
          )
        ) : <Form name="login" onFinish={handleSubmit} autoComplete="off" size="large">
          {authError && <Alert data-testid="login-error" type="error" showIcon message={t('login.failed')} style={{ marginBottom: 16 }} />}
          <Form.Item
            name="username"
            rules={[{ required: true, message: t('login.required', { field: t('login.username') }) }]}
          >
            <Input
              data-testid="login-username"
              prefix={<UserOutlined style={{ color: 'var(--ec-color-text-subtle)' }} />}
              placeholder={t('login.username')}
            />
          </Form.Item>
          <Form.Item
            name="password"
            rules={[{ required: true, message: t('login.required', { field: t('login.password') }) }]}
          >
            <Input.Password
              data-testid="login-password"
              prefix={<LockOutlined style={{ color: 'var(--ec-color-text-subtle)' }} />}
              placeholder={t('login.password')}
            />
          </Form.Item>
          <Form.Item style={{ marginBottom: 0 }}>
            <Button data-testid="login-submit" type="primary" htmlType="submit" block loading={loading}>
              {t('login.submit')}
            </Button>
          </Form.Item>
        </Form>}
      </div>
    </div>
  )
}

export default LoginPage
