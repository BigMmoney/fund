import { AlertTriangle, CheckCircle2, Info, ShieldAlert } from 'lucide-react'
import type { ReactNode } from 'react'

type Tone = 'neutral' | 'success' | 'warning' | 'danger'

const toneMap: Record<Tone, { icon: typeof Info; shell: string; badge: string }> = {
  neutral: {
    icon: Info,
    shell: 'surface-card',
    badge: 'bg-white',
  },
  success: {
    icon: CheckCircle2,
    shell: 'surface-card bg-[linear-gradient(180deg,#ffffff_0%,#f7f7f5_100%)]',
    badge: 'bg-neutral-50',
  },
  warning: {
    icon: AlertTriangle,
    shell: 'surface-card bg-[linear-gradient(180deg,#ffffff_0%,#faf8f2_100%)]',
    badge: 'bg-[#faf7ee]',
  },
  danger: {
    icon: ShieldAlert,
    shell: 'surface-card bg-[linear-gradient(180deg,#ffffff_0%,#faf5f5_100%)]',
    badge: 'bg-[#faf2f2]',
  },
}

export function StatusBanner({
  tone = 'neutral',
  eyebrow,
  title,
  message,
  trailing,
  compact = false,
}: {
  tone?: Tone
  eyebrow?: string
  title?: string
  message: string
  trailing?: ReactNode
  compact?: boolean
}) {
  const { icon: Icon, shell, badge } = toneMap[tone]

  return (
    <div className={`${shell} ${compact ? 'px-4 py-3 md:px-5' : 'px-6 py-6 md:px-7'}`}>
      <div className={`flex flex-col ${compact ? 'gap-2 md:flex-row md:items-center md:justify-between' : 'gap-5 md:flex-row md:items-start md:justify-between'}`}>
        <div className={`flex min-w-0 items-start ${compact ? 'gap-3' : 'gap-4'}`}>
          <div className={`flex ${compact ? 'h-8 w-8 rounded-[14px]' : 'h-11 w-11 rounded-2xl'} shrink-0 items-center justify-center border border-black ${badge}`}>
            <Icon className="h-4 w-4" />
          </div>
          <div className="min-w-0">
            {eyebrow ? <div className="eyebrow">{eyebrow}</div> : null}
            {title ? <div className={`${compact ? 'mt-0.5 text-[15px]' : 'mt-2 text-lg'} font-semibold tracking-[-0.03em] text-black`}>{title}</div> : null}
            <div className={`${compact ? 'text-xs leading-6 md:text-sm md:leading-6' : 'premium-copy'} text-neutral-700 ${title ? (compact ? 'mt-0.5' : 'mt-2') : ''}`}>{message}</div>
          </div>
        </div>
        {trailing ? <div className="shrink-0 self-start">{trailing}</div> : null}
      </div>
    </div>
  )
}
