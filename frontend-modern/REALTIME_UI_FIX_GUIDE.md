# 实时 UI 闪烁/NaN/状态错乱 - 完整诊断与修复方案

---

## 🔴 根因诊断结果 (已确认)

### 问题 1: 快讯永远只有 4 条，没有历史滚动
**根因**: `generateBreakingNews()` 函数（第 3639 行）返回**硬编码的 4 条**模拟新闻
```typescript
// NewsIntelligence.tsx:3639-3700
const generateBreakingNews = (): BreakingNews[] => {
  return [
    { id: 'bn-1', headline: 'Trump announces...' },
    { id: 'bn-2', headline: 'ECB Emergency...' },
    { id: 'bn-3', headline: 'PBoC announces...' },
    { id: 'bn-4', headline: 'DoD confirms...' }  // 只有 4 条！
  ]
}
```
**修复方案**: 扩展生成器生成 20+ 条初始新闻 + 实时追加 + 历史分页

### 问题 2: 政策时间轴只有 3 条
**根因**: `generateMockTimelines()` 函数（第 3707 行）返回**硬编码的 3 条**模拟时间轴
```typescript
// NewsIntelligence.tsx:3707-3900
const generateMockTimelines = (): PolicyTimeline[] => {
  return [
    { policyId: 'us-china-tariff-2026', ... },
    { policyId: 'eu-dma-compliance', ... },
    { policyId: 'china-export-control', ... }  // 只有 3 条！
  ]
}
```
**修复方案**: 扩展生成器生成 6+ 条多样化时间轴

### 问题 3: UI 闪烁 (DOM 卸载/重挂载)
**根因**: `setXXX(newArray)` 每次调用都创建新数组引用 → React Reconciler 无法 diff → 全部重渲染
```typescript
// 问题代码模式（多处）:
setBreakingNews(prev => [newNews, ...prev].slice(0, 50))  // 每次新数组 = 列表项全部 unmount/mount
setAlerts(newAlerts)  // 替换整个数组 = 闪烁
```
**修复方案**: 使用 `useStableList` hook + `RealtimeBuffer` 缓冲高频更新

### 问题 4: NaN 出现在 Signal Line
**根因**: 直接进行数学运算，未防护边界情况（除零、空数组、undefined）
**修复方案**: 全局使用 `safeDiv()`, `safeAvg()` 等安全数学函数（已创建于 safemath.ts）

### 问题 5: Score=0 但 State=confirmed (状态-评分解耦)
**根因**: 状态(state)和分数(score)独立更新，没有绑定约束
**修复方案**: 使用 `BoundStateMachine` 强制 score→state 映射（已创建于 boundStateMachine.ts）

---

## 📊 问题分类

### A. React 渲染模型 - 导致 Unmount/Mount 的行为

| 现象 | 根因 | 映射到闪烁区域 | 级别 |
|------|------|---------------|------|
| 列表项消失又出现 | `key` 不稳定（使用 index 或随机值） | 新闻卡片、时间轴圆点 | 架构级 |
| 整个组件重新挂载 | 父组件 state 变化导致条件渲染路径切换 | Header 状态区域 | 实现级 |
| 动画中断重置 | CSS transition 被打断（元素被替换） | L0/L0.5 颜色条 | 实现级 |
| 图标闪烁 | 图标组件每次渲染创建新对象 | ↑↓ 方向图标 | 实现级 |
| 统计条闪烁 | 父组件 re-render 触发所有子组件 re-render | 顶部统计条 | 架构级 |

### B. NaN 产生的数学与工程原因

```
┌─────────────────────────────────────────────────────────────────┐
│                    NaN 产生路径分析                              │
├─────────────────────────────────────────────────────────────────┤
│ 1. 除零错误                                                      │
│    percentChange = (current - previous) / previous              │
│    当 previous = 0 → NaN                                        │
│                                                                  │
│ 2. 窗口未对齐                                                    │
│    score6h = calcScore(data.slice(-6h))                         │
│    当 data 为空或长度不足 → reduce() 返回 undefined → NaN        │
│                                                                  │
│ 3. 初始值缺失                                                    │
│    const [score, setScore] = useState<number>() // undefined    │
│    display: score.toFixed(2) → NaN                              │
│                                                                  │
│ 4. JSON 解析                                                     │
│    JSON.parse 后数字字段可能为 null/undefined                    │
│    直接运算 → NaN                                                │
│                                                                  │
│ 5. 聚合函数边界                                                  │
│    Math.min(...[]) → Infinity                                   │
│    Math.max(...[]) → -Infinity                                  │
│    avg([]) → 0/0 → NaN                                          │
└─────────────────────────────────────────────────────────────────┘
```

### 防护规则

**后端规则**：
```go
// Go 示例 - 响应序列化前检查
func sanitizeResponse(data interface{}) interface{} {
    switch v := data.(type) {
    case float64:
        if math.IsNaN(v) || math.IsInf(v, 0) {
            return 0 // 或 null
        }
    case map[string]interface{}:
        for k, val := range v {
            v[k] = sanitizeResponse(val)
        }
    }
    return data
}
```

**前端规则**：
```typescript
// 已实现于 src/lib/safemath.ts
export function safeDivide(a: number, b: number, fallback = 0): number {
  if (b === 0 || !Number.isFinite(b) || Math.abs(b) < 1e-10) return fallback
  const result = a / b
  return Number.isFinite(result) ? result : fallback
}
```

---

## C. 标准数据流架构

```
┌───────────────────────────────────────────────────────────────────────────────┐
│                        WS → Buffer → Aggregation → Store → UI                 │
├───────────────────────────────────────────────────────────────────────────────┤
│                                                                               │
│   ┌─────────┐     ┌──────────────┐     ┌────────────────┐     ┌────────────┐ │
│   │   WS    │────▶│   Buffer     │────▶│  Aggregation   │────▶│   Store    │ │
│   │ onmsg   │     │  (高频允许)   │     │  (节流必须)     │     │  (低频)    │ │
│   └─────────┘     └──────────────┘     └────────────────┘     └────────────┘ │
│       ↑                  │                     │                     │       │
│       │               push()              flush()              setState()    │
│       │            每条消息               500ms/次              触发渲染     │
│       │                                                              │       │
│       │                                                              ▼       │
│       │                                                       ┌──────────┐   │
│       │                                                       │    UI    │   │
│       │                                                       │ (React)  │   │
│       │                                                       └──────────┘   │
│       │                                                                       │
│   WebSocket 消息                                                              │
│   1-100ms/条                                                                  │
│                                                                               │
├───────────────────────────────────────────────────────────────────────────────┤
│ 层级说明：                                                                     │
│ • WS层：允许任意高频，只做接收                                                  │
│ • Buffer层：允许高频 push，内存缓冲，去重                                       │
│ • Aggregation层：必须节流，500-1000ms flush 一次                               │
│ • Store层：低频更新，配合 React.memo / selector                                │
│ • UI层：只响应 Store 变化，不直接订阅 WS                                        │
└───────────────────────────────────────────────────────────────────────────────┘
```

### 关键实现（已创建）

```typescript
// src/lib/realtimeBuffer.ts
import { RealtimeBuffer, SignalHysteresis } from '@/lib/realtimeBuffer'

// 创建缓冲器
const newsBuffer = new RealtimeBuffer({
  flushIntervalMs: 500,    // 500ms 刷新一次
  maxBufferSize: 200,
  dedupeKey: 'id'
})

// WS 接收（高频）
ws.onmessage = (event) => {
  const data = JSON.parse(event.data)
  newsBuffer.push(data)  // ✅ 不触发 setState
}

// 订阅批量更新（低频）
newsBuffer.subscribe((items) => {
  setNews(prev => mergeItems(prev, items))  // ✅ 批量更新
})
```

---

## D. 最小改动修复方案（止血版）

### D1. WS Buffer + Flush 频率

```typescript
// 修改 NewsIntelligence.tsx 中的实时更新逻辑

// Before (问题代码)
useEffect(() => {
  const interval = setInterval(() => {
    if (Math.random() < 0.15) {
      setBreakingNews(prev => [newNews, ...prev])  // ❌ 高频 setState
    }
  }, 5000)
}, [])

// After (修复代码)
import { newsBuffer } from '@/lib/realtimeBuffer'

useEffect(() => {
  // 启动缓冲器
  newsBuffer.start()
  
  // 订阅批量更新
  const unsubscribe = newsBuffer.subscribe((items) => {
    setBreakingNews(prev => {
      const merged = [...items, ...prev].slice(0, 50)
      return merged
    })
  })
  
  // 模拟数据（生产环境改为 WS）
  const interval = setInterval(() => {
    if (Math.random() < 0.15) {
      newsBuffer.push(generateNews())  // ✅ 只 push，不 setState
    }
  }, 5000)
  
  return () => {
    newsBuffer.stop()
    unsubscribe()
    clearInterval(interval)
  }
}, [])
```

### D2. Stable Key / 列表结构共享

```typescript
// Before (问题代码)
{topics.map((topic, index) => (
  <TopicCard key={index} topic={topic} />  // ❌ index 作为 key
))}

// After (修复代码)
import { useStableList } from '@/hooks/useStableList'

const stableTopics = useStableList(topics, {
  idKey: 'id',
  maxItems: 100,
  newItemDuration: 3000
})

{stableTopics.map(({ key, data, isNew }) => (
  <TopicCard 
    key={key}           // ✅ 稳定的 key
    topic={data} 
    isNew={isNew}       // 可用于高亮动画
  />
))}
```

### D3. Signal 迟钝化（Window + Hysteresis）

```typescript
// Before (问题代码)
const direction = velocity > 0 ? 'up' : 'down'  // ❌ 瞬时值，频繁切换

// After (修复代码)
import { SignalHysteresis } from '@/lib/realtimeBuffer'

const velocityHysteresis = useMemo(() => new SignalHysteresis({
  windowMs: 5000,
  threshold: 10,  // 10% 变化才切换
  minSamples: 3
}), [])

// 在更新时
const { shouldUpdate, stableValue } = velocityHysteresis.addSample(rawVelocity)

// stableValue 是稳定后的值，不会频繁跳变
const direction = stableValue > 5 ? 'up' : stableValue < -5 ? 'down' : 'neutral'
```

### D4. React.memo / Selector 粒度

```typescript
// 统计条组件优化
const StatsBar = React.memo(function StatsBar({ 
  total, l0, l05, l1, l2 
}: StatsBarProps) {
  return (
    <div className="flex gap-4">
      <StatItem label="Total" value={total} />
      <StatItem label="L0" value={l0} />
      {/* ... */}
    </div>
  )
}, (prev, next) => {
  // 自定义比较：只有数值变化才 re-render
  return prev.total === next.total &&
         prev.l0 === next.l0 &&
         prev.l05 === next.l05 &&
         prev.l1 === next.l1 &&
         prev.l2 === next.l2
})

// 使用 selector 提取细粒度数据
const selectStats = (state) => ({
  total: state.topics.length,
  l0: state.topics.filter(t => t.l0Count > 0).length,
  // ...
})
```

---

## E. 评分 → 状态机绑定方案

### 核心规则

```
┌────────────────────────────────────────────────────────────────────────────┐
│                          评分-状态强绑定规则                                │
├────────────────────────────────────────────────────────────────────────────┤
│ Score Range    │ State             │ canTrade │ 说明                       │
├────────────────┼───────────────────┼──────────┼────────────────────────────┤
│ < 0            │ FROZEN            │ false    │ 异常值，冻结               │
│ 0 - 19.99      │ MONITORING        │ false    │ 监控中，无行动             │
│ 20 - 34.99     │ SIGNAL_DETECTED   │ false    │ 信号已检测                 │
│ 35 - 49.99     │ ANALYZING         │ false    │ 分析中                     │
│ 50 - 64.99     │ ACTIONABLE        │ true     │ 可行动 ← 最低可交易门槛    │
│ 65 - 79.99     │ EXECUTING         │ true     │ 执行中                     │
│ 80 - 100       │ CONFIRMED         │ true     │ 已确认                     │
│ wasHigh + <20  │ EXHAUSTED         │ false    │ 曾高分现低分，已耗尽       │
└────────────────────────────────────────────────────────────────────────────┘
```

### 为什么 score=0 不能进入 CONFIRMED

```typescript
// src/lib/boundStateMachine.ts 核心逻辑

private deriveState(score: number, wasHighScore: boolean): BoundPolicyState {
  // 规则 1: score 必须 >= 80 才能是 CONFIRMED
  if (score >= 80) return 'confirmed'
  
  // 规则 2: score 必须 >= 65 才能是 EXECUTING
  if (score >= 65) return 'executing'
  
  // 规则 3: score = 0/null/NaN 必须回退
  if (score < 20) {
    return wasHighScore ? 'exhausted' : 'monitoring'
  }
  
  // ... 其他状态
}

// 使用示例
const machine = new BoundStateMachine()
machine.updateScore(85)  // → CONFIRMED ✅
machine.updateScore(0)   // → EXHAUSTED (曾高分) ✅
machine.updateScore(NaN) // → 内部净化为 0 → EXHAUSTED ✅
```

### 验证一致性

```typescript
// 调试工具
const { valid, errors } = machine.validate()
if (!valid) {
  console.error('[StateMachine] 状态不一致:', errors)
  // 自动修复
  machine.reset()
}
```

---

## F. 三种变化类型的 UI 行为

| 变化类型 | 定义 | UI 行为 | 实现 |
|----------|------|---------|------|
| **数值变化** | score: 65 → 68 | 平滑过渡，无闪烁 | CSS transition: 0.3s |
| **等级变化** | level: L1 → L0 | 短暂高亮 + 动画 | 0.5s flash animation |
| **状态变化** | state: analyzing → executing | 状态条平滑切换 | 无闪烁，颜色渐变 |

### 实现

```typescript
// src/lib/realtimeBuffer.ts - classifyChange 函数
import { classifyChange } from '@/lib/realtimeBuffer'

const change = classifyChange(prevTopic, nextTopic, 
  ['sourceLevel', 'severity'],  // 等级字段
  ['state', 'policyState']       // 状态字段
)

if (change.type === 'value') {
  // 数值变化：使用 CSS transition
  return <div className="transition-all duration-300">{value}</div>
}

if (change.type === 'grade' && change.shouldFlash) {
  // 等级变化：短暂闪烁
  return <div className="animate-flash">{level}</div>
}

if (change.type === 'state') {
  // 状态变化：平滑动画
  return <div className="transition-colors duration-500">{state}</div>
}
```

### CSS 动画

```css
/* 添加到 index.css */
@keyframes flash {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.5; background: rgba(59, 130, 246, 0.3); }
}

.animate-flash {
  animation: flash 0.5s ease-in-out;
}

/* 数值变化平滑过渡 */
.value-transition {
  transition: all 0.3s ease-out;
}

/* 状态颜色渐变 */
.state-transition {
  transition: background-color 0.5s ease, color 0.3s ease;
}
```

---

## G. 修复检查清单 (Checklist)

### 🔍 闪烁检查

- [ ] **统计条**：切换 tab 或收到新数据时，Total/L0/L0.5/L1/L2 数字不闪烁
- [ ] **新闻卡片颜色条**：L0/L0.5 颜色条保持稳定，不频繁闪烁
- [ ] **时间轴圆点**：圆点颜色/状态平滑过渡，不消失重现
- [ ] **方向图标**：↑↓ 图标不频繁切换（需持续 5s+ 才切换）
- [ ] **Header 状态区**：刷新时间/状态不闪烁

### 🔢 NaN 检查

- [ ] Signal Line 不显示 `+NaN%` 或 `-NaN%`
- [ ] 所有评分显示为有效数字或 `--`
- [ ] 百分比变化显示为有效数字或 `--`
- [ ] 控制台无 `NaN` 相关警告

### 📊 状态一致性检查

- [ ] score = 0 时，状态不能是 CONFIRMED/EXECUTING
- [ ] score < 50 时，canTrade = false
- [ ] 状态变化有明确的评分支撑
- [ ] 状态机 validate() 返回 valid = true

### ⚡ 性能检查

- [ ] React DevTools Profiler：无不必要的 re-render
- [ ] 列表组件使用 stable key（非 index）
- [ ] 高频数据使用 buffer（500ms flush）
- [ ] 大列表使用虚拟化（> 50 条）

### 🧪 测试场景

```typescript
// 1. 高频更新测试
for (let i = 0; i < 100; i++) {
  buffer.push({ id: `test-${i}`, score: Math.random() * 100 })
}
// 预期：UI 只更新 1-2 次，无闪烁

// 2. NaN 注入测试
machine.updateScore(NaN)
machine.updateScore(undefined)
machine.updateScore(Infinity)
// 预期：状态正确降级，无崩溃

// 3. 快速状态切换测试
machine.updateScore(85)
machine.updateScore(15)
machine.updateScore(90)
// 预期：状态平滑过渡，无抖动
```

---

## 📁 新增文件清单

| 文件 | 用途 |
|------|------|
| `src/lib/realtimeBuffer.ts` | 数据缓冲、信号迟钝化、变化分类 |
| `src/lib/boundStateMachine.ts` | 评分-状态强绑定状态机 |
| `src/hooks/useStableList.ts` | 稳定列表渲染 Hook |

## 🔧 待修改文件

| 文件 | 修改内容 |
|------|----------|
| `src/pages/NewsIntelligence.tsx` | 接入 buffer、stable list、bound state machine |
| `src/index.css` | 添加平滑过渡动画样式 |
| `src/components/SafeDisplay.tsx` | 已有，使用 classifyChange 优化 |
