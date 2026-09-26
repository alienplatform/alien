# SandboxBaseImageRepository

## Example Usage

```typescript
import { SandboxBaseImageRepository } from "@alienplatform/platform-api/models";

let value: SandboxBaseImageRepository = {
  registryHost: "<value>",
  repository: "<value>",
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `registryHost`                                               | *string*                                                     | :heavy_check_mark:                                           | Manager registry host to `docker login` to and push through. |
| `repository`                                                 | *string*                                                     | :heavy_check_mark:                                           | This project's repository under that host.                   |