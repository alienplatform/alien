# PlanDeploymentComputeNetworkUnion


## Supported Types

### `operations.PlanDeploymentComputeNetworkUseDefault`

```typescript
const value: operations.PlanDeploymentComputeNetworkUseDefault = {
  type: "use-default",
};
```

### `operations.PlanDeploymentComputeNetworkCreate`

```typescript
const value: operations.PlanDeploymentComputeNetworkCreate = {
  type: "create",
};
```

### `operations.PlanDeploymentComputeNetworkByoVpcAws`

```typescript
const value: operations.PlanDeploymentComputeNetworkByoVpcAws = {
  privateSubnetIds: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  publicSubnetIds: [
    "<value 1>",
    "<value 2>",
  ],
  type: "byo-vpc-aws",
  vpcId: "<id>",
};
```

### `operations.PlanDeploymentComputeNetworkByoVpcGcp`

```typescript
const value: operations.PlanDeploymentComputeNetworkByoVpcGcp = {
  networkName: "<value>",
  region: "<value>",
  subnetName: "<value>",
  type: "byo-vpc-gcp",
};
```

### `operations.PlanDeploymentComputeNetworkByoVnetAzure`

```typescript
const value: operations.PlanDeploymentComputeNetworkByoVnetAzure = {
  privateSubnetName: "<value>",
  publicSubnetName: "<value>",
  type: "byo-vnet-azure",
  vnetResourceId: "<id>",
};
```

### `any`

```typescript
const value: any = "<value>";
```

