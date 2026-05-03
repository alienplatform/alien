"use client"

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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Area, AreaChart, CartesianGrid, ResponsiveContainer, XAxis } from "recharts"

interface MetricsHistoryChartProps {
  data: Array<{
    date: string
    totalPRs: number
    reviewTime: number
  }>
}

const chartConfig = {
  totalPRs: {
    label: "Total PRs",
    color: "var(--primary)",
  },
  reviewTime: {
    label: "Review Time (h)",
    color: "var(--chart-2)",
  },
} satisfies ChartConfig

export function MetricsHistoryChart({ data }: MetricsHistoryChartProps) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>Metrics Over Time</CardTitle>
        <CardDescription>PR volume and review time trends</CardDescription>
        <CardAction>
          <Select defaultValue="30d">
            <SelectTrigger className="w-32" size="sm">
              <SelectValue placeholder="Select range" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="7d">Last 7 days</SelectItem>
              <SelectItem value="30d">Last 30 days</SelectItem>
              <SelectItem value="90d">Last 90 days</SelectItem>
            </SelectContent>
          </Select>
        </CardAction>
      </CardHeader>
      <CardContent>
        <ChartContainer config={chartConfig} className="h-[300px] w-full">
          <ResponsiveContainer width="100%" height="100%">
            <AreaChart data={data} margin={{ left: 12, right: 12 }}>
              <defs>
                <linearGradient id="fillPRs" x1="0" y1="0" x2="0" y2="1">
                  <stop offset="5%" stopColor="var(--color-totalPRs)" stopOpacity={0.8} />
                  <stop offset="95%" stopColor="var(--color-totalPRs)" stopOpacity={0.1} />
                </linearGradient>
              </defs>
              <CartesianGrid vertical={false} strokeDasharray="3 3" />
              <XAxis
                dataKey="date"
                tickLine={false}
                axisLine={false}
                tickMargin={8}
                minTickGap={32}
                tickFormatter={value => {
                  const date = new Date(value)
                  return date.toLocaleDateString("en-US", {
                    month: "short",
                    day: "numeric",
                  })
                }}
              />
              <ChartTooltip
                cursor={false}
                content={
                  <ChartTooltipContent
                    labelFormatter={value => {
                      return new Date(value).toLocaleDateString("en-US", {
                        month: "short",
                        day: "numeric",
                      })
                    }}
                    indicator="dot"
                  />
                }
              />
              <Area
                dataKey="totalPRs"
                type="natural"
                fill="url(#fillPRs)"
                stroke="var(--color-totalPRs)"
                strokeWidth={2}
              />
            </AreaChart>
          </ResponsiveContainer>
        </ChartContainer>
      </CardContent>
    </Card>
  )
}
