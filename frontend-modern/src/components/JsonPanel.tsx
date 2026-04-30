export function JsonPanel({ title, value }: { title: string; value: unknown }) {
  return (
    <div className="json-card">
      <div className="json-card-title">{title}</div>
      <pre>{JSON.stringify(value, null, 2)}</pre>
    </div>
  )
}
