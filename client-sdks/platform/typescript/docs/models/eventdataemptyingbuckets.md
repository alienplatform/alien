# EventDataEmptyingBuckets

## Example Usage

```typescript
import { EventDataEmptyingBuckets } from "@alienplatform/platform-api/models";

let value: EventDataEmptyingBuckets = {
  bucketNames: [],
  type: "EmptyingBuckets",
};
```

## Fields

| Field                                 | Type                                  | Required                              | Description                           |
| ------------------------------------- | ------------------------------------- | ------------------------------------- | ------------------------------------- |
| `bucketNames`                         | *string*[]                            | :heavy_check_mark:                    | Names of the S3 buckets being emptied |
| `type`                                | *"EmptyingBuckets"*                   | :heavy_check_mark:                    | N/A                                   |
