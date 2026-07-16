# DataDownloadingAlienWorkerRuntime

## Example Usage

```typescript
import { DataDownloadingAlienWorkerRuntime } from "@alienplatform/platform-api/models";

let value: DataDownloadingAlienWorkerRuntime = {
  targetTriple: "<value>",
  type: "DownloadingAlienWorkerRuntime",
  url: "https://decent-toaster.info",
};
```

## Fields

| Field                             | Type                              | Required                          | Description                       |
| --------------------------------- | --------------------------------- | --------------------------------- | --------------------------------- |
| `targetTriple`                    | *string*                          | :heavy_check_mark:                | Target triple for the runtime     |
| `type`                            | *"DownloadingAlienWorkerRuntime"* | :heavy_check_mark:                | N/A                               |
| `url`                             | *string*                          | :heavy_check_mark:                | URL being downloaded from         |