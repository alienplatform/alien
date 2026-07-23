# EventDataDownloadingAlienRuntime

## Example Usage

```typescript
import { EventDataDownloadingAlienRuntime } from "@alienplatform/platform-api/models";

let value: EventDataDownloadingAlienRuntime = {
  targetTriple: "<value>",
  type: "DownloadingAlienRuntime",
  url: "https://unlawful-skyline.com",
};
```

## Fields

| Field                         | Type                          | Required                      | Description                   |
| ----------------------------- | ----------------------------- | ----------------------------- | ----------------------------- |
| `targetTriple`                | *string*                      | :heavy_check_mark:            | Target triple for the runtime |
| `type`                        | *"DownloadingAlienRuntime"*   | :heavy_check_mark:            | N/A                           |
| `url`                         | *string*                      | :heavy_check_mark:            | URL being downloaded from     |
