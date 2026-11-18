import { useMemo } from 'react';
import { levers } from '@/engine';
import { Table, Td, Th } from '@/ui';
import type { ViewProps } from '../types';
import { ViewPlaceholder } from '../ViewPlaceholder';

export default function WhatMattersView({ profile }: ViewProps) {
  // The levers are real: the engine derives them from this plan. Their effects are
  // not shown until the sweep that measures them is built.
  const list = useMemo(() => {
    try {
      return levers(profile);
    } catch {
      return [];
    }
  }, [profile]);
  return (
    <ViewPlaceholder
      question="What does the result depend on most?"
      intro="Each lever is pushed low and high while every simulated life faces the same markets, so the difference is the lever and not luck. Things you control are kept apart from things you do not."
      charts={[
        { title: 'What you control', shows: 'Retirement age, spending, savings rate, claiming age and housing choices, each as a bar with its interval, so small effects are visibly indistinguishable from zero.' },
        { title: 'What you do not', shows: 'Stock returns, inflation, longevity, healthcare costs and job loss, each over a stated range.' },
        { title: 'Trade-offs', shows: 'Levers expressed in each other\'s units: working this many more months does about as much as cutting this much spending.' },
      ]}
    >
      {list.length > 0 && (
        <div className="mt-8">
          <p className="eyebrow mb-2">Levers the engine will test for this plan</p>
          <Table>
            <thead>
              <tr>
                <Th>Lever</Th>
                <Th>Low setting</Th>
                <Th>High setting</Th>
              </tr>
            </thead>
            <tbody>
              {list.map((lever) => (
                <tr key={lever.id}>
                  <Td className="font-medium text-ink-1">{lever.label}</Td>
                  <Td className="text-ink-2">{lever.low_label}</Td>
                  <Td className="text-ink-2">{lever.high_label}</Td>
                </tr>
              ))}
            </tbody>
          </Table>
        </div>
      )}
    </ViewPlaceholder>
  );
}
