# ConfigureProjectSourceRequestBody


## Supported Types

### `operations.ConfigureProjectSourceRepository`

```typescript
const value: operations.ConfigureProjectSourceRepository = {
  mode: "repository",
  gitRepository: {
    type: "github",
    repo: "alien/my-agent",
  },
};
```
### `operations.TemplateRequest`

```typescript
const value: operations.TemplateRequest = {
  mode: "template",
  targetNamespace: "<value>",
  templatePath: "examples/customer-models-ts",
};
```
