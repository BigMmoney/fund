import { LineChart, Line, AreaChart, Area, BarChart, Bar, XAxis, YAxis, CartesianGrid, Tooltip, ResponsiveContainer } from 'recharts'

interface ChartComponentProps {
  data: any[]
  type: string
  height: number
}

export function ChartComponent({ data, type, height }: ChartComponentProps) {
  if (!data || data.length === 0) {
    return (
      <div className="flex items-center justify-center h-[400px] text-muted-foreground">
        Loading chart data...
      </div>
    )
  }

  const formatData = data.map(d => ({
    ...d,
    time: new Date(d.time).toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit' })
  }))

  const CustomTooltip = ({ active, payload }: any) => {
    if (active && payload && payload.length) {
      return (
        <div className="bg-card border border-border rounded-lg p-3 shadow-lg">
          <p className="text-sm font-semibold">{payload[0].payload.time}</p>
          <p className="text-sm text-success">
            Price: ${payload[0].value.toFixed(2)}
          </p>
          {payload[0].payload.volume && (
            <p className="text-sm text-muted-foreground">
              Vol: ${(payload[0].payload.volume / 1000000).toFixed(2)}M
            </p>
          )}
        </div>
      )
    }
    return null
  }

  switch (type) {
    case 'line':
      return (
        <ResponsiveContainer width="100%" height={height}>
          <AreaChart data={formatData}>
            <defs>
              <linearGradient id="colorPrice" x1="0" y1="0" x2="0" y2="1">
                <stop offset="5%" stopColor="#3b82f6" stopOpacity={0.3}/>
                <stop offset="95%" stopColor="#3b82f6" stopOpacity={0}/>
              </linearGradient>
            </defs>
            <CartesianGrid strokeDasharray="3 3" stroke="#333" />
            <XAxis dataKey="time" stroke="#888" fontSize={12} />
            <YAxis stroke="#888" fontSize={12} domain={['auto', 'auto']} />
            <Tooltip content={<CustomTooltip />} />
            <Area 
              type="monotone" 
              dataKey="price" 
              stroke="#3b82f6" 
              fill="url(#colorPrice)"
              strokeWidth={2}
            />
          </AreaChart>
        </ResponsiveContainer>
      )

    case 'candle':
      // Simplified candlestick using bars
      return (
        <ResponsiveContainer width="100%" height={height}>
          <BarChart data={formatData}>
            <CartesianGrid strokeDasharray="3 3" stroke="#333" />
            <XAxis dataKey="time" stroke="#888" fontSize={12} />
            <YAxis stroke="#888" fontSize={12} />
            <Tooltip content={<CustomTooltip />} />
            <Bar dataKey="high" fill="#3fb950" />
            <Bar dataKey="low" fill="#f85149" />
          </BarChart>
        </ResponsiveContainer>
      )

    case 'volume':
      return (
        <ResponsiveContainer width="100%" height={height}>
          <BarChart data={formatData}>
            <CartesianGrid strokeDasharray="3 3" stroke="#333" />
            <XAxis dataKey="time" stroke="#888" fontSize={12} />
            <YAxis stroke="#888" fontSize={12} />
            <Tooltip content={<CustomTooltip />} />
            <Bar dataKey="volume" fill="#8b5cf6" />
          </BarChart>
        </ResponsiveContainer>
      )

    case 'depth':
    case 'heatmap':
      return (
        <ResponsiveContainer width="100%" height={height}>
          <AreaChart data={formatData}>
            <CartesianGrid strokeDasharray="3 3" stroke="#333" />
            <XAxis dataKey="time" stroke="#888" fontSize={12} />
            <YAxis stroke="#888" fontSize={12} />
            <Tooltip content={<CustomTooltip />} />
            <Area 
              type="monotone" 
              dataKey="price" 
              stroke="#f59e0b" 
              fill="#f59e0b"
              fillOpacity={0.6}
            />
          </AreaChart>
        </ResponsiveContainer>
      )

    default:
      return (
        <ResponsiveContainer width="100%" height={height}>
          <LineChart data={formatData}>
            <CartesianGrid strokeDasharray="3 3" stroke="#333" />
            <XAxis dataKey="time" stroke="#888" fontSize={12} />
            <YAxis stroke="#888" fontSize={12} />
            <Tooltip content={<CustomTooltip />} />
            <Line type="monotone" dataKey="price" stroke="#3b82f6" strokeWidth={2} dot={false} />
          </LineChart>
        </ResponsiveContainer>
      )
  }
}
