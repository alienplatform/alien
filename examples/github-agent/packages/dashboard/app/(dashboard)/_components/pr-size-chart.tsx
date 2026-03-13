"use client"

import Link from "next/link"
import { Pie, PieChart, Cell, ResponsiveContainer, Legend } from "recharts"
import { Card, CardContent, CardDescription, CardHeader, CardTitle, CardAction } from "@/components/ui/card"
import { Button } from "@/components/ui/button"
import { IconEye } from "@tabler/icons-react"
import {
  ChartContainer,
  ChartTooltip,
  ChartTooltipContent,
  type ChartConfig,
} from "@/components/ui/chart"

interface PrSizeChartProps {
  data: {
    small: number
    medium: number
    large: number
  }
  integrationId: string
  agentId: string
  repoName?: string
}

const chartConfig = {
  small: {
    label: "Small",
    color: "oklch(0.723 0.219 149.579)",
  },
  medium: {
    label: "Medium",
    color: "oklch(0.795 0.184 86.047)",
  },
  large: {
    label: "Large",
    color: "oklch(0.637 0.237 25.331)",
  },
} satisfies ChartConfig

export function PrSizeChart({ data, integrationId, agentId, repoName }: PrSizeChartProps) {
  const chartData = [
    { name: "Small", value: data.small, fill: chartConfig.small.color },
    { name: "Medium", value: data.medium, fill: chartConfig.medium.color },
    { name: "Large", value: data.large, fill: chartConfig.large.color },
  ].filter((item) => item.value > 0)

  const total = data.small + data.medium + data.large

  return (
    <Card>
      <CardHeader>
        <CardTitle>PR Size Distribution</CardTitle>
        <CardDescription>
          Pull requests categorized by code change size
        </CardDescription>
        <CardAction>
          <Link href={`/pull-requests?integrationId=${encodeURIComponent(integrationId)}&agentId=${encodeURIComponent(agentId)}&repo=${encodeURIComponent(repoName || "")}`}>
            <Button variant="outline" size="sm">
              <IconEye className="h-3.5 w-3.5 mr-1.5" />
              View PRs
            </Button>
          </Link>
        </CardAction>
      </CardHeader>
      <CardContent>
        <ChartContainer config={chartConfig} className="mx-auto aspect-square h-[280px]">
          <ResponsiveContainer width="100%" height="100%">
            <PieChart>
              <ChartTooltip
                cursor={false}
                content={<ChartTooltipContent hideLabel />}
              />
              <Pie
                data={chartData}
                dataKey="value"
                nameKey="name"
                cx="50%"
                cy="50%"
                innerRadius={60}
                outerRadius={100}
                strokeWidth={2}
                stroke="var(--background)"
              >
                {chartData.map((entry) => (
                  <Cell key={entry.name} fill={entry.fill} />
                ))}
              </Pie>
            </PieChart>
          </ResponsiveContainer>
        </ChartContainer>
        <div className="flex justify-center gap-6 mt-4">
          {chartData.map((item) => (
            <div key={item.name} className="flex items-center gap-2">
              <div
                className="h-3 w-3 rounded-full"
                style={{ backgroundColor: item.fill }}
              />
              <span className="text-sm text-muted-foreground">
                {item.name}: {item.value} ({total > 0 ? Math.round((item.value / total) * 100) : 0}%)
              </span>
            </div>
          ))}
        </div>
      </CardContent>
    </Card>
  )
}

