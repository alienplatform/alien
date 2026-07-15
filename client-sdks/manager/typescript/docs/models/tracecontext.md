# TraceContext

W3C Trace Context propagated with a command lease.

The values are kept in their standard wire form so receivers can attach
them to handler telemetry without inventing separate trace/span fields.

## Example Usage

```typescript
import { TraceContext } from "@alienplatform/manager-api/models";

let value: TraceContext = {
  traceparent: "<value>",
};
```

## Fields

| Field                                   | Type                                    | Required                                | Description                             |
| --------------------------------------- | --------------------------------------- | --------------------------------------- | --------------------------------------- |
| `traceparent`                           | *string*                                | :heavy_check_mark:                      | W3C `traceparent` header value.         |
| `tracestate`                            | *string*                                | :heavy_minus_sign:                      | Optional W3C `tracestate` header value. |