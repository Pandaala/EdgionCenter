import { useState, useEffect } from 'react'
import { useNavigate } from 'react-router-dom'
import { Form, Input, Button, message } from 'antd'
import { UserOutlined, LockOutlined } from '@ant-design/icons'
import { authApi } from '../../api/auth'
import { setLoggedIn, isLoggedIn } from '../../utils/auth'
import { getAppMode } from '../../utils/proxy'
import { useT } from '../../i18n/index.tsx'

const LoginPage = () => {
  const navigate = useNavigate()
  const t = useT()
  const [loading, setLoading] = useState(false)
  const isCenter = getAppMode() === 'center'

  useEffect(() => {
    if (isLoggedIn()) {
      navigate('/', { replace: true })
    }
  }, [navigate])

  const handleSubmit = async (values: { username: string; password: string }) => {
    setLoading(true)
    try {
      const res = await authApi.login({ username: values.username, password: values.password })
      if (res.success && res.data) {
        setLoggedIn()
        navigate('/', { replace: true })
      } else {
        message.error(t('login.failed'))
      }
    } catch {
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
        <Form name="login" onFinish={handleSubmit} autoComplete="off" size="large">
          <Form.Item
            name="username"
            rules={[{ required: true, message: t('login.required', { field: t('login.username') }) }]}
          >
            <Input
              prefix={<UserOutlined style={{ color: 'var(--ec-color-text-subtle)' }} />}
              placeholder={t('login.username')}
            />
          </Form.Item>
          <Form.Item
            name="password"
            rules={[{ required: true, message: t('login.required', { field: t('login.password') }) }]}
          >
            <Input.Password
              prefix={<LockOutlined style={{ color: 'var(--ec-color-text-subtle)' }} />}
              placeholder={t('login.password')}
            />
          </Form.Item>
          <Form.Item style={{ marginBottom: 0 }}>
            <Button type="primary" htmlType="submit" block loading={loading}>
              {t('login.submit')}
            </Button>
          </Form.Item>
        </Form>
      </div>
    </div>
  )
}

export default LoginPage
