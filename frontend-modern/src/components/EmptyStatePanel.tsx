import type { ReactNode } from 'react'

export function EmptyStatePanel({
  title,
  description,
  action,
  compact = false,
}: {
  title: string
  description: string
  action?: ReactNode
  compact?: boolean
}) {
  return (
    <div
      className={`surface-dashed flex h-full min-h-0 flex-col justify-center text-center ${
        compact ? 'px-5 py-6 md:px-6 md:py-7' : 'px-6 py-8 md:px-8 md:py-10'
      }`}
    >
      <div className="mx-auto mb-4 flex items-center gap-2">
        <span className="h-2 w-2 rounded-full border border-black bg-black" />
        <span className="h-2 w-2 rounded-full border border-black/60 bg-white" />
        <span className="h-2 w-2 rounded-full border border-black/35 bg-neutral-100" />
      </div>
      <div className={`${compact ? 'text-base' : 'text-lg'} font-semibold tracking-[-0.03em] text-black`}>{title}</div>
      <div className={`mx-auto mt-3 max-w-xl ${compact ? 'text-sm leading-7' : 'text-[15px] leading-8'} text-neutral-600`}>{description}</div>
      <div className={`mx-auto mt-5 grid w-full max-w-2xl gap-2 ${compact ? 'sm:grid-cols-2' : 'sm:grid-cols-3'}`}>
        <div className="rounded-[16px] border border-black/80 bg-white px-4 py-3">
          <div className="text-[10px] uppercase tracking-[0.16em] text-neutral-500">状态</div>
          <div className="mt-2 text-sm font-semibold tracking-[-0.02em] text-black">等待数据进入</div>
        </div>
        <div className="rounded-[16px] border border-black/80 bg-white px-4 py-3">
          <div className="text-[10px] uppercase tracking-[0.16em] text-neutral-500">上下文</div>
          <div className="mt-2 text-sm font-semibold tracking-[-0.02em] text-black">保留当前工作区</div>
        </div>
        {!compact ? (
          <div className="rounded-[16px] border border-black/80 bg-white px-4 py-3">
            <div className="text-[10px] uppercase tracking-[0.16em] text-neutral-500">链路</div>
            <div className="mt-2 text-sm font-semibold tracking-[-0.02em] text-black">可继续刷新或执行</div>
          </div>
        ) : null}
      </div>
      {action ? <div className="mt-5 flex justify-center">{action}</div> : null}
    </div>
  )
}
