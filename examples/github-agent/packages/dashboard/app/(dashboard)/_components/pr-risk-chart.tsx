"use client"

import { Button } from "@/components/ui/button"
import {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import {
  type ChartConfig,
  ChartContainer,
  ChartTooltip,
  ChartTooltipContent,
} from "@/components/ui/chart"
import { IconEye } from "@tabler/icons-react"
import Link from "next/link"
import { Bar, BarChart, CartesianGrid, Cell, ResponsiveContainer, XAxis, YAxis } from "recharts"

interface PrRiskChartProps {
  data: {
    low: number
    medium: number
    high: number
    critical: number
  }
  integrationId: string
  agentId: string
  repoName?: string
}

const chartConfig = {
  low: {
    label: "Low",
    color: "oklch(0.723 0.219 149.579)",
  },
  medium: {
    label: "Medium",
    color: "oklch(0.795 0.184 86.047)",
  },
  high: {
    label: "High",
    color: "oklch(0.705 0.213 47.604)",
  },
  critical: {
    label: "Critical",
    color: "oklch(0.637 0.237 25.331)",
  },
} satisfies ChartConfig

export function PrRiskChart({ data, integrationId, agentId, repoName }: PrRiskChartProps) {
  const chartData = [
    { name: "Low", value: data.low, fill: chartConfig.low.color },
    { name: "Medium", value: data.medium, fill: chartConfig.medium.color },
    { name: "High", value: data.high, fill: chartConfig.high.color },
    { name: "Critical", value: data.critical, fill: chartConfig.critical.color },
  ]

  return (
    <Card>
      <CardHeader>
        <CardTitle>Risk Distribution</CardTitle>
        <CardDescription>Pull requests categorized by risk level</CardDescription>
        <CardAction>
          <Link
            href={`/pull-requests?integrationId=${encodeURIComponent(integrationId)}&agentId=${encodeURIComponent(agentId)}&repo=${encodeURIComponent(repoName || "")}`}
          >
            <Button variant="outline" size="sm">
              <IconEye className="h-3.5 w-3.5 mr-1.5" />
              View PRs
            </Button>
          </Link>
        </CardAction>
      </CardHeader>
      <CardContent>
        <ChartContainer config={chartConfig} className="h-[280px] w-full">
          <ResponsiveContainer width="100%" height="100%">
            <BarChart data={chartData} layout="vertical" margin={{ left: 0, right: 16 }}>
              <CartesianGrid horizontal={false} strokeDasharray="3 3" />
              <XAxis type="number" tickLine={false} axisLine={false} />
              <YAxis
                dataKey="name"
                type="category"
                tickLine={false}
                axisLine={false}
                width={70}
                tick={{ fontSize: 12 }}
              />
              <ChartTooltip
                cursor={{ fill: "var(--muted)", opacity: 0.3 }}
                content={<ChartTooltipContent hideLabel />}
              />
              <Bar dataKey="value" radius={[0, 4, 4, 0]}>
                {chartData.map((entry, index) => (
                  <Cell key={`cell-${index}`} fill={entry.fill} />
                ))}
              </Bar>
            </BarChart>
          </ResponsiveContainer>
        </ChartContainer>
        <div className="flex justify-center gap-4 mt-4 flex-wrap">
          {chartData.map(item => (
            <div key={item.name} className="flex items-center gap-2">
              <div className="h-3 w-3 rounded-sm" style={{ backgroundColor: item.fill }} />
              <span className="text-sm text-muted-foreground">
                {item.name}: {item.value}
              </span>
            </div>
          ))}
        </div>
      </CardContent>
    </Card>
  )
}
