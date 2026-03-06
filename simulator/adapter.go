package simulator

type ControlAction struct {
	TargetBatchWindowSteps *int `json:"target_batch_window_steps,omitempty"`
}

type ActionSpec struct {
	SupportsBatchWindowControl bool `json:"supports_batch_window_control"`
	MinBatchWindowSteps        int  `json:"min_batch_window_steps"`
	MaxBatchWindowSteps        int  `json:"max_batch_window_steps"`
}

type RewardWeights struct {
	FillWeight         float64 `json:"fill_weight"`
	SpreadPenalty      float64 `json:"spread_penalty"`
	ArbitragePenalty   float64 `json:"arbitrage_penalty"`
	RiskRejectPenalty  float64 `json:"risk_reject_penalty"`
	ConservationPenalty float64 `json:"conservation_penalty"`
}

type MetricsDelta struct {
	FillsDelta                 int     `json:"fills_delta"`
	SpreadDelta                float64 `json:"spread_delta"`
	ArbitrageProfitDelta       float64 `json:"arbitrage_profit_delta"`
	RiskRejectionsDelta        int     `json:"risk_rejections_delta"`
	ConservationBreachesDelta  int     `json:"conservation_breaches_delta"`
}

type AdapterInfo struct {
	ScenarioName          string       `json:"scenario_name"`
	AppliedAction         ControlAction `json:"applied_action"`
	ActionSpec            ActionSpec   `json:"action_spec"`
	MetricsDelta          MetricsDelta `json:"metrics_delta"`
	CurrentBatchWindowMs  int          `json:"current_batch_window_ms"`
}

type AdapterTimestep struct {
	Observation Observation     `json:"observation"`
	Metrics     MetricsSnapshot `json:"metrics"`
	Reward      float64         `json:"reward"`
	Done        bool            `json:"done"`
	Info        AdapterInfo     `json:"info"`
}

type Adapter struct {
	env           *Environment
	rewardWeights RewardWeights
	prevMetrics   MetricsSnapshot
}

func NewAdapter(cfg ScenarioConfig) *Adapter {
	env := NewEnvironment(cfg)
	return &Adapter{
		env:           env,
		rewardWeights: defaultRewardWeights(),
		prevMetrics:   env.Metrics(),
	}
}

func defaultRewardWeights() RewardWeights {
	return RewardWeights{
		FillWeight:          1.0,
		SpreadPenalty:       0.25,
		ArbitragePenalty:    0.005,
		RiskRejectPenalty:   0.5,
		ConservationPenalty: 10.0,
	}
}

func (a *Adapter) Reset() AdapterTimestep {
	observation := a.env.Reset()
	metrics := a.env.Metrics()
	a.prevMetrics = metrics
	return AdapterTimestep{
		Observation: observation,
		Metrics:     metrics,
		Reward:      0,
		Done:        observation.Done,
		Info: AdapterInfo{
			ScenarioName:         a.env.cfg.Name,
			AppliedAction:        ControlAction{},
			ActionSpec:           a.ActionSpec(),
			MetricsDelta:         MetricsDelta{},
			CurrentBatchWindowMs: observation.CurrentBatchWindowStep * int(a.env.cfg.StepDuration.Milliseconds()),
		},
	}
}

func (a *Adapter) Step(action ControlAction) AdapterTimestep {
	appliedAction := a.applyAction(action)
	result := a.env.Step()
	delta := metricsDelta(a.prevMetrics, result.Metrics)
	reward := a.computeReward(delta)
	a.prevMetrics = result.Metrics
	return AdapterTimestep{
		Observation: result.Observation,
		Metrics:     result.Metrics,
		Reward:      reward,
		Done:        result.Observation.Done,
		Info: AdapterInfo{
			ScenarioName:         a.env.cfg.Name,
			AppliedAction:        appliedAction,
			ActionSpec:           a.ActionSpec(),
			MetricsDelta:         delta,
			CurrentBatchWindowMs: result.Observation.CurrentBatchWindowStep * int(a.env.cfg.StepDuration.Milliseconds()),
		},
	}
}

func (a *Adapter) Observe() AdapterTimestep {
	observation := a.env.Observe()
	metrics := a.env.Metrics()
	return AdapterTimestep{
		Observation: observation,
		Metrics:     metrics,
		Reward:      0,
		Done:        observation.Done,
		Info: AdapterInfo{
			ScenarioName:         a.env.cfg.Name,
			AppliedAction:        ControlAction{},
			ActionSpec:           a.ActionSpec(),
			MetricsDelta:         MetricsDelta{},
			CurrentBatchWindowMs: observation.CurrentBatchWindowStep * int(a.env.cfg.StepDuration.Milliseconds()),
		},
	}
}

func (a *Adapter) ActionSpec() ActionSpec {
	spec := ActionSpec{}
	if a.env.cfg.Mode == ModeAdaptiveBatch {
		spec.SupportsBatchWindowControl = true
		spec.MinBatchWindowSteps = maxInt(1, a.env.cfg.AdaptiveMinWindowSteps)
		spec.MaxBatchWindowSteps = maxInt(spec.MinBatchWindowSteps, a.env.cfg.AdaptiveMaxWindowSteps)
	}
	return spec
}

func (a *Adapter) applyAction(action ControlAction) ControlAction {
	spec := a.ActionSpec()
	if !spec.SupportsBatchWindowControl || action.TargetBatchWindowSteps == nil {
		return ControlAction{}
	}
	target := *action.TargetBatchWindowSteps
	if target < spec.MinBatchWindowSteps {
		target = spec.MinBatchWindowSteps
	}
	if target > spec.MaxBatchWindowSteps {
		target = spec.MaxBatchWindowSteps
	}
	a.env.currentBatchWindow = target
	return ControlAction{TargetBatchWindowSteps: &target}
}

func (a *Adapter) computeReward(delta MetricsDelta) float64 {
	return a.rewardWeights.FillWeight*float64(delta.FillsDelta) -
		a.rewardWeights.SpreadPenalty*delta.SpreadDelta -
		a.rewardWeights.ArbitragePenalty*delta.ArbitrageProfitDelta -
		a.rewardWeights.RiskRejectPenalty*float64(delta.RiskRejectionsDelta) -
		a.rewardWeights.ConservationPenalty*float64(delta.ConservationBreachesDelta)
}

func metricsDelta(prev, next MetricsSnapshot) MetricsDelta {
	return MetricsDelta{
		FillsDelta:                next.Fills - prev.Fills,
		SpreadDelta:               next.AverageSpread - prev.AverageSpread,
		ArbitrageProfitDelta:      next.LatencyArbitrageProfit - prev.LatencyArbitrageProfit,
		RiskRejectionsDelta:       next.RiskRejections - prev.RiskRejections,
		ConservationBreachesDelta: next.ConservationBreaches - prev.ConservationBreaches,
	}
}
