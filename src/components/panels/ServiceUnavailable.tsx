import { useTranslation } from 'react-i18next'

interface ServiceUnavailableProps {
  message: string
  onNavigate?: () => void
}

// ponytail: single shared component for all "not installed / stopped" alerts
export default function ServiceUnavailable({ message, onNavigate }: ServiceUnavailableProps) {
  const { t } = useTranslation()
  return (
    <div className="alert alert-error">
      <div style={{ marginBottom: '12px', fontSize: '14px' }}>
        {message}
      </div>
      {onNavigate && (
        <button className="btn-primary" onClick={onNavigate}>
          {t('common.goToSoftwareRepo')}
        </button>
      )}
    </div>
  )
}
