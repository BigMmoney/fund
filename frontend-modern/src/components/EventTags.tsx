import React from 'react';
import { MarketEvent } from '../lib/tradingLogic';

interface EventTagsProps {
  events: MarketEvent[];
  compact?: boolean;
}

export const EventTags: React.FC<EventTagsProps> = ({ events, compact = false }) => {
  if (!events || events.length === 0) {
    return null;
  }

  const getSeverityColor = (severity: string): string => {
    switch (severity) {
      case 'critical':
        return 'bg-red-900 border-red-700 text-red-300';
      case 'warning':
        return 'bg-yellow-900 border-yellow-700 text-yellow-300';
      case 'info':
      default:
        return 'bg-blue-900 border-blue-700 text-blue-300';
    }
  };

  const getSeverityIcon = (severity: string): string => {
    switch (severity) {
      case 'critical':
        return '🔴';
      case 'warning':
        return '🟠';
      case 'info':
      default:
        return '🔵';
    }
  };

  const displayEvents = compact ? events.slice(0, 3) : events;

  return (
    <div className="flex flex-wrap gap-2">
      {displayEvents.map((event) => (
        <div
          key={event.id}
          className={`inline-flex items-center gap-2 px-3 py-2 rounded-lg border text-sm font-medium transition-all ${getSeverityColor(event.severity)}`}
          title={event.description}
        >
          <span>{getSeverityIcon(event.severity)}</span>
          <span>{event.label}</span>
        </div>
      ))}
      {compact && events.length > 3 && (
        <div className="px-3 py-2 rounded-lg border bg-slate-700 border-slate-600 text-slate-300 text-sm font-medium">
          +{events.length - 3} more
        </div>
      )}
    </div>
  );
};

export default EventTags;
