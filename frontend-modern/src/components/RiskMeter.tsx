import React, { useMemo } from 'react';
import { calculateRiskMetrics } from '../lib/tradingLogic';

interface RiskMeterProps {
  position: {
    avgCostPrice: number;
    currentPrice: number;
    quantity: number;
    leverage: number;
    equity: number;
    side: 'LONG' | 'SHORT';
  };
  showDetails?: boolean;
}

export const RiskMeter: React.FC<RiskMeterProps> = ({ 
  position, 
  showDetails = true 
}) => {
  const metrics = useMemo(() => {
    return calculateRiskMetrics(
      position.avgCostPrice,
      position.currentPrice,
      position.quantity,
      position.leverage,
      position.equity,
      position.side
    );
  }, [position]);

  const getRiskLevelColor = (level: string): string => {
    switch (level) {
      case 'LOW':
        return 'text-green-400';
      case 'MEDIUM':
        return 'text-yellow-400';
      case 'HIGH':
        return 'text-red-400';
      case 'LIQUIDATION':
        return 'text-red-200 animate-pulse';
      default:
        return 'text-white';
    }
  };

  const getBarColor = (level: string): string => {
    switch (level) {
      case 'LOW':
        return 'bg-green-500';
      case 'MEDIUM':
        return 'bg-yellow-500';
      case 'HIGH':
        return 'bg-red-500';
      case 'LIQUIDATION':
        return 'bg-red-700 animate-pulse';
      default:
        return 'bg-blue-500';
    }
  };

  const getBackgroundColor = (level: string): string => {
    switch (level) {
      case 'LOW':
        return 'bg-green-900';
      case 'MEDIUM':
        return 'bg-yellow-900';
      case 'HIGH':
        return 'bg-red-900';
      case 'LIQUIDATION':
        return 'bg-red-950';
      default:
        return 'bg-slate-700';
    }
  };

  const formatPrice = (price: number) => {
    return new Intl.NumberFormat('en-US', {
      style: 'currency',
      currency: 'USD',
      minimumFractionDigits: 2,
      maximumFractionDigits: 2,
    }).format(price);
  };

  return (
    <div className="bg-slate-700 rounded-lg p-4 border border-slate-600">
      <h3 className="text-lg font-semibold text-white mb-4">Risk Metrics</h3>

      {/* Liquidation Price - Most Important */}
      <div className="mb-4">
        <div className="flex justify-between items-center mb-2">
          <span className="text-sm text-slate-400">🎯 Liquidation Price</span>
          <span className={`text-xl font-bold ${getRiskLevelColor(metrics.riskLevel)}`}>
            {formatPrice(metrics.liquidationPrice)}
          </span>
        </div>
        <div className="flex justify-between text-xs text-slate-500">
          <span>Entry: {formatPrice(position.avgCostPrice)}</span>
          <span>Current: {formatPrice(position.currentPrice)}</span>
        </div>
        
        {/* Distance to Liquidation */}
        <div className="mt-2 p-2 rounded bg-slate-600">
          <div className="flex justify-between items-center">
            <span className="text-xs text-slate-400">Distance to Liquidation</span>
            <span className={`text-sm font-bold ${
              metrics.distanceToLiquidation > 15 ? 'text-green-400' :
              metrics.distanceToLiquidation > 5 ? 'text-yellow-400' :
              'text-red-400'
            }`}>
              {metrics.distanceToLiquidation.toFixed(2)}%
            </span>
          </div>
        </div>
      </div>

      {/* Margin Ratio Bar */}
      <div className="mb-4">
        <div className="flex justify-between items-center mb-2">
          <span className="text-sm text-slate-400">Margin Ratio</span>
          <span className={`text-lg font-bold ${getRiskLevelColor(metrics.riskLevel)}`}>
            {metrics.marginRatio.toFixed(1)}%
          </span>
        </div>
        
        {/* Risk Bar with Gradient */}
        <div className="relative w-full h-6 bg-slate-600 rounded overflow-hidden">
          <div
            className={`h-full transition-all duration-300 ${getBarColor(metrics.riskLevel)}`}
            style={{ width: `${Math.min(metrics.marginRatio, 100)}%` }}
          />
          {/* Threshold markers */}
          <div className="absolute top-0 left-[50%] h-full w-px bg-yellow-400/50" />
          <div className="absolute top-0 left-[75%] h-full w-px bg-orange-400/50" />
          <div className="absolute top-0 left-[95%] h-full w-px bg-red-400/50" />
        </div>

        {/* Risk Zones Labels */}
        <div className="flex justify-between text-[10px] text-slate-500 mt-1">
          <span className="text-green-400">LOW</span>
          <span className="text-yellow-400">MEDIUM</span>
          <span className="text-red-400">HIGH</span>
          <span className="text-red-200">⚠️</span>
        </div>
      </div>

      {/* Risk Level Status */}
      <div className={`p-3 rounded ${getBackgroundColor(metrics.riskLevel)}`}>
        <div className="flex items-center justify-between">
          <div>
            <p className="text-xs text-slate-400">Risk Status</p>
            <p className={`text-lg font-bold ${getRiskLevelColor(metrics.riskLevel)}`}>
              {metrics.riskLevel}
              {metrics.riskLevel === 'LIQUIDATION' && ' 🚨'}
              {metrics.riskLevel === 'HIGH' && ' ⚠️'}
            </p>
          </div>
          {metrics.riskLevel === 'HIGH' || metrics.riskLevel === 'LIQUIDATION' ? (
            <div className="text-right">
              <p className="text-xs text-red-400">Action Required</p>
              <p className="text-[10px] text-slate-400">Reduce position or add margin</p>
            </div>
          ) : null}
        </div>
      </div>

      {/* Details */}
      {showDetails && (
        <div className="mt-4 space-y-2 text-sm">
          <div className="flex justify-between">
            <span className="text-slate-400">Margin Used</span>
            <span className="text-white font-mono">{formatPrice(metrics.marginUsed)}</span>
          </div>
          <div className="flex justify-between">
            <span className="text-slate-400">Margin Available</span>
            <span className="text-white font-mono">{formatPrice(metrics.marginAvailable)}</span>
          </div>
          <div className="flex justify-between">
            <span className="text-slate-400">Max Loss (to Liquidation)</span>
            <span className="text-red-400 font-mono">-{formatPrice(metrics.maxLoss)}</span>
          </div>
          <div className="flex justify-between">
            <span className="text-slate-400">Leverage</span>
            <span className="text-white font-mono">{position.leverage.toFixed(1)}x</span>
          </div>
          <div className="flex justify-between">
            <span className="text-slate-400">Position Size</span>
            <span className="text-white font-mono">{position.quantity.toFixed(4)}</span>
          </div>
          <div className="flex justify-between">
            <span className="text-slate-400">Side</span>
            <span className={position.side === 'LONG' ? 'text-green-400' : 'text-red-400'}>
              {position.side}
            </span>
          </div>
        </div>
      )}
    </div>
  );
};

export default RiskMeter;
