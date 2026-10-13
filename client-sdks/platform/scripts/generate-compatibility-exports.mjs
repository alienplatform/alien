import { readFile, readdir, writeFile } from "node:fs/promises";

// Speakeasy may regroup inline models when optional fields are added. Preserve
// published deep import paths while using the current generated response parsers.
const modules = {
  "createmanagerresponseproviderawsalb2": "createmanagerresponseprovidergkegatewayenum2",
  "createmanagerresponsedomainsunion2": "createmanagerresponsepublicendpointtargetunion2",
  "deploymentdetailresponsependingpreparedstacktypeunion": "deploymentdetailresponsependingpreparedstacktypeenvenum",
  "deploymentdetailresponsepreparedstackoperationsunion": "deploymentdetailresponsepreparedstackoperations",
  "deploymentpendingpreparedstacktypeunion": "deploymentpendingpreparedstacktypeenvenum",
  "deploymentpreparedstackoperationsunion": "deploymentpreparedstackoperations",
  "managerretryresponseendpointaccess2": "managerretryresponsedomains2",
  "managerretryresponseproviderunion2": "managerretryresponseprovidergkegateway2",
  "persistimporteddeploymentrequestpendingpreparedstackcustomsettingsunion": "persistimporteddeploymentrequestpendingpreparedstacksettingscustom1",
  "persistimporteddeploymentrequestpreparedstackoverridestackconditionunion": "persistimporteddeploymentrequestpreparedstackoverrideconditionstack",
  "synclistresponsependingpreparedstacktypeunion": "synclistresponsependingpreparedstacktypeenvenum",
  "synclistresponsepreparedstackoperationsunion": "synclistresponsepreparedstackoperations",
  "targetdeploymentconfig": "targetdeploymentvolumerestore",
  "releaseinfotypestringlist": "targetdeploymentconfig"
};
// Names can move separately from the shared models in their former module.
const additionalExports = {
  "targetdeploymentconfig": {
    "targetdeployment": [
      "TargetDeploymentConfig",
      "TargetDeploymentConfig$inboundSchema",
      "targetDeploymentConfigFromJSON"
]
  },
  "createmanagerresponseproviderawsalb2": {
    "createmanagerresponse": [
      "CreateManagerResponseProviderAwsAlb2",
      "CreateManagerResponseProviderAwsAlb2$inboundSchema",
      "CreateManagerResponseProviderAwsAlbEnum2",
      "CreateManagerResponseProviderAwsAlbEnum2$inboundSchema",
      "CreateManagerResponseProviderGkeGateway2",
      "CreateManagerResponseProviderGkeGateway2$inboundSchema",
      "createManagerResponseProviderAwsAlb2FromJSON",
      "createManagerResponseProviderGkeGateway2FromJSON"
    ]
  },
  "createmanagerresponsedomainsunion2": {
    "createmanagerresponseprovidergkegatewayenum2": [
      "CreateManagerResponseDomains2",
      "CreateManagerResponseDomains2$inboundSchema",
      "CreateManagerResponseDomainsUnion2",
      "CreateManagerResponseDomainsUnion2$inboundSchema",
      "createManagerResponseDomains2FromJSON",
      "createManagerResponseDomainsUnion2FromJSON"
    ]
  },
  "deploymentdetailresponsependingpreparedstacktypeunion": {
    "deploymentdetailresponsepreparedstackoperations": [
      "DeploymentDetailResponsePendingPreparedStackTypeUnion",
      "DeploymentDetailResponsePendingPreparedStackTypeUnion$inboundSchema",
      "deploymentDetailResponsePendingPreparedStackTypeUnionFromJSON"
    ]
  },
  "deploymentdetailresponsepreparedstackoperationsunion": {
    "deploymentdetailresponse": [
      "DeploymentDetailResponsePreparedStackOperationsUnion",
      "DeploymentDetailResponsePreparedStackOperationsUnion$inboundSchema",
      "deploymentDetailResponsePreparedStackOperationsUnionFromJSON"
    ]
  },
  "deploymentpendingpreparedstacktypeunion": {
    "deploymentpreparedstackoperations": [
      "DeploymentPendingPreparedStackTypeUnion",
      "DeploymentPendingPreparedStackTypeUnion$inboundSchema",
      "deploymentPendingPreparedStackTypeUnionFromJSON"
    ]
  },
  "deploymentpreparedstackoperationsunion": {
    "deployment": [
      "DeploymentPreparedStackOperationsUnion",
      "DeploymentPreparedStackOperationsUnion$inboundSchema",
      "deploymentPreparedStackOperationsUnionFromJSON"
    ]
  },
  "managerretryresponseendpointaccess2": {
    "managerretryresponseprovidergkegateway2": [
      "ManagerRetryResponseDomainsUnion2",
      "ManagerRetryResponseDomainsUnion2$inboundSchema",
      "ManagerRetryResponseEndpointAccess2",
      "ManagerRetryResponseEndpointAccess2$inboundSchema",
      "managerRetryResponseDomainsUnion2FromJSON"
    ]
  },
  "managerretryresponseproviderunion2": {
    "managerretryresponse": [
      "ManagerRetryResponseProviderAwsAlb2",
      "ManagerRetryResponseProviderAwsAlb2$inboundSchema",
      "ManagerRetryResponseProviderAwsAlbEnum2",
      "ManagerRetryResponseProviderAwsAlbEnum2$inboundSchema",
      "ManagerRetryResponseProviderUnion2",
      "ManagerRetryResponseProviderUnion2$inboundSchema",
      "managerRetryResponseProviderAwsAlb2FromJSON",
      "managerRetryResponseProviderUnion2FromJSON"
    ]
  },
  "persistimporteddeploymentrequestpendingpreparedstackcustomsettingsunion": {
    "persistimporteddeploymentrequestpreparedstackoverrideconditionstack": [
      "PersistImportedDeploymentRequestPendingPreparedStackCustomSettingsUnion",
      "PersistImportedDeploymentRequestPendingPreparedStackCustomSettingsUnion$Outbound",
      "PersistImportedDeploymentRequestPendingPreparedStackCustomSettingsUnion$outboundSchema",
      "persistImportedDeploymentRequestPendingPreparedStackCustomSettingsUnionToJSON"
    ]
  },
  "persistimporteddeploymentrequestpreparedstackoverridestackconditionunion": {
    "persistimporteddeploymentrequest": [
      "PersistImportedDeploymentRequestPreparedStackOverrideStackConditionUnion",
      "PersistImportedDeploymentRequestPreparedStackOverrideStackConditionUnion$Outbound",
      "PersistImportedDeploymentRequestPreparedStackOverrideStackConditionUnion$outboundSchema",
      "persistImportedDeploymentRequestPreparedStackOverrideStackConditionUnionToJSON"
    ]
  },
  "synclistresponsependingpreparedstacktypeunion": {
    "synclistresponsepreparedstackoperations": [
      "SyncListResponsePendingPreparedStackTypeUnion",
      "SyncListResponsePendingPreparedStackTypeUnion$inboundSchema",
      "syncListResponsePendingPreparedStackTypeUnionFromJSON"
    ]
  },
  "synclistresponsepreparedstackoperationsunion": {
    "synclistresponse": [
      "SyncListResponsePreparedStackOperationsUnion",
      "SyncListResponsePreparedStackOperationsUnion$inboundSchema",
      "syncListResponsePreparedStackOperationsUnionFromJSON"
    ]
  },
  "releaseinfotypestringlist": {
    "targetdeployment": [
      "ReleaseInfoTypeStringList",
      "ReleaseInfoTypeStringList$inboundSchema"
    ]
  }
};
// Authority settings add fields to inline groups; retain their published module paths.
Object.assign(modules, {
  "createmanagerresponseprovidergkegatewayenum2": "createmanagerresponseproviderazureapplicationgatewayforcontainersenum2",
  "createmanagerresponsepublicendpointtargetunion2": "createmanagerresponsepublicendpointtargetmachineaddresses2",
  "deploymentdetailresponsependingpreparedstacktypeenvenum": "deploymentdetailresponsependingpreparedstackdefaultunion",
  "deploymentdetailresponsepreparedstackoperations": "deploymentdetailresponsepreparedstackplugins",
  "deploymentpendingpreparedstacktypeenvenum": "deploymentpendingpreparedstackdefaultunion",
  "deploymentpreparedstackoperations": "deploymentpreparedstackplugins",
  "managerretryresponseprovidergkegateway2": "managerretryresponseproviderazureapplicationgatewayforcontainers2",
  "managerretryresponsedomains2": "managerretryresponsepublicendpointtargetunion2",
  "persistimporteddeploymentrequestpendingpreparedstacksettingscustom1": "persistimporteddeploymentrequestpendingpreparedstacksettingscustom2",
  "persistimporteddeploymentrequestpreparedstackoverrideconditionstack": "persistimporteddeploymentrequestpreparedstackoverridegcpresource",
  "synclistresponsependingpreparedstacktypeenvenum": "synclistresponsependingpreparedstackdefaultunion",
  "synclistresponsepreparedstackoperations": "synclistresponsepreparedstackplugins"
});
Object.assign(additionalExports, {
  "createmanagerresponseprovidergkegatewayenum2": {
    "createmanagerresponse": [
      "CreateManagerResponseProviderAzureApplicationGatewayForContainers2",
      "CreateManagerResponseProviderAzureApplicationGatewayForContainers2$inboundSchema",
      "CreateManagerResponseProviderGkeGatewayEnum2",
      "CreateManagerResponseProviderGkeGatewayEnum2$inboundSchema",
      "createManagerResponseProviderAzureApplicationGatewayForContainers2FromJSON"
    ]
  },
  "createmanagerresponsepublicendpointtargetunion2": {
    "createmanagerresponseproviderazureapplicationgatewayforcontainersenum2": [
      "CreateManagerResponsePublicEndpointTargetUnion2",
      "CreateManagerResponsePublicEndpointTargetUnion2$inboundSchema",
      "createManagerResponsePublicEndpointTargetUnion2FromJSON"
    ]
  },
  "deploymentdetailresponsependingpreparedstacktypeenvenum": {
    "deploymentdetailresponsepreparedstackplugins": [
      "DeploymentDetailResponsePendingPreparedStackTypeEnvEnum",
      "DeploymentDetailResponsePendingPreparedStackTypeEnvEnum$inboundSchema"
    ]
  },
  "deploymentdetailresponsepreparedstackoperations": {
    "deploymentdetailresponse": [
      "DeploymentDetailResponsePreparedStackOperations",
      "DeploymentDetailResponsePreparedStackOperations$inboundSchema",
      "deploymentDetailResponsePreparedStackOperationsFromJSON"
    ]
  },
  "deploymentpendingpreparedstacktypeenvenum": {
    "deploymentpreparedstackplugins": [
      "DeploymentPendingPreparedStackTypeEnvEnum",
      "DeploymentPendingPreparedStackTypeEnvEnum$inboundSchema"
    ]
  },
  "deploymentpreparedstackoperations": {
    "deployment": [
      "DeploymentPreparedStackOperations",
      "DeploymentPreparedStackOperations$inboundSchema",
      "deploymentPreparedStackOperationsFromJSON"
    ]
  },
  "managerretryresponseprovidergkegateway2": {
    "managerretryresponse": [
      "ManagerRetryResponseProviderGkeGateway2",
      "ManagerRetryResponseProviderGkeGateway2$inboundSchema",
      "ManagerRetryResponseProviderGkeGatewayEnum2",
      "ManagerRetryResponseProviderGkeGatewayEnum2$inboundSchema",
      "managerRetryResponseProviderGkeGateway2FromJSON"
    ]
  },
  "managerretryresponsedomains2": {
    "managerretryresponseproviderazureapplicationgatewayforcontainers2": [
      "ManagerRetryResponseDomains2",
      "ManagerRetryResponseDomains2$inboundSchema",
      "managerRetryResponseDomains2FromJSON"
    ]
  },
  "persistimporteddeploymentrequestpendingpreparedstacksettingscustom1": {
    "persistimporteddeploymentrequestpreparedstackoverridegcpresource": [
      "PersistImportedDeploymentRequestPendingPreparedStackSettingsCustom1",
      "PersistImportedDeploymentRequestPendingPreparedStackSettingsCustom1$Outbound",
      "PersistImportedDeploymentRequestPendingPreparedStackSettingsCustom1$outboundSchema",
      "persistImportedDeploymentRequestPendingPreparedStackSettingsCustom1ToJSON"
    ]
  },
  "persistimporteddeploymentrequestpreparedstackoverrideconditionstack": {
    "persistimporteddeploymentrequest": [
      "PersistImportedDeploymentRequestPreparedStackOverrideConditionStack",
      "PersistImportedDeploymentRequestPreparedStackOverrideConditionStack$Outbound",
      "PersistImportedDeploymentRequestPreparedStackOverrideConditionStack$outboundSchema",
      "persistImportedDeploymentRequestPreparedStackOverrideConditionStackToJSON"
    ]
  },
  "synclistresponsependingpreparedstacktypeenvenum": {
    "synclistresponsepreparedstackplugins": [
      "SyncListResponsePendingPreparedStackTypeEnvEnum",
      "SyncListResponsePendingPreparedStackTypeEnvEnum$inboundSchema"
    ]
  },
  "synclistresponsepreparedstackoperations": {
    "synclistresponse": [
      "SyncListResponsePreparedStackOperations",
      "SyncListResponsePreparedStackOperations$inboundSchema",
      "syncListResponsePreparedStackOperationsFromJSON"
    ]
  }
});
const models = new URL("../typescript/src/models/", import.meta.url);
async function exportIsValue(module, name) {
  if (modules[module]) {
    const moved = Object.entries(additionalExports[module] ?? {})
      .find(([, names]) => names.includes(name));
    return exportIsValue(moved?.[0] ?? modules[module], name);
  }
  const source = await readFile(new URL(`${module}.ts`, models), "utf8");
  return [...source.matchAll(/export\s+(?:const|function|class)\s+([\w$]+)/g)]
    .some(match => match[1] === name);
}

// Generate leaf forwarders before older compatibility paths that reference them.
const orderedModules = Object.entries(modules).sort(([, left], [, right]) =>
  Number(left in modules) - Number(right in modules));
for (const [previous, current] of orderedModules) {
  const existing = await readFile(new URL(`${previous}.ts`, models), "utf8").catch(() => "");
  if (existing && !existing.startsWith("// Generated by generate-compatibility-exports.mjs.")) continue;
  await readFile(new URL(`${current}.ts`, models));
  const reexports = [];
  for (const [module, names] of Object.entries(additionalExports[previous] ?? {})) {
    for (const name of names) {
      const value = await exportIsValue(module, name);
      reexports.push(`export ${value ? "" : "type "}{ ${name} } from "./${module}.js";`);
    }
  }
  await writeFile(
    new URL(`${previous}.ts`, models),
    `// Generated by generate-compatibility-exports.mjs. DO NOT EDIT.\nexport * from "./${current}.js";\n${reexports.join("\n")}\n`,
  );
}

// Preserve serializers and types reported by older runtimes when the generator
// qualifies their names with the enclosing request. Root and deep imports both
// continue to expose the existing names.
const resourceMarker = "\n// Published resource-name compatibility exports.\n";
for (const file of await readdir(models)) {
  if (!file.endsWith(".ts")) continue;
  const url = new URL(file, models);
  const original = await readFile(url, "utf8");
  const source = original.split(resourceMarker)[0];
  const declarations = [...source.matchAll(/export\s+(type|const|function)\s+((?:SyncReconcileRequest(?:CpuUnion|MemoryUnion|Container)\d+(?:\$Outbound|\$outboundSchema)?|syncReconcileRequest(?:CpuUnion|MemoryUnion|Container)\d+ToJSON))(?![\w$])/g)];
  const aliases = declarations.map(([, kind, name]) => {
    const alias = name.startsWith("SyncReconcileRequest")
      ? name.slice("SyncReconcileRequest".length)
      : name.slice("syncReconcileRequest".length).replace(/^[A-Z]/, value => value.toLowerCase());
    return `export ${kind === "type" ? "type " : ""}{ ${name} as ${alias} };`;
  });
  if (aliases.length) await writeFile(url, `${source}${resourceMarker}${aliases.join("\n")}\n`);
}

// Inline setup settings share enums with deployment info after regeneration.
// Preserve the public deployment-info names at both root and deep import paths.
const deploymentInfoUrl = new URL("deploymentinfo.ts", models);
const deploymentInfoMarker = "\n// Published deployment-info compatibility exports.\n";
const deploymentInfoSource = (await readFile(deploymentInfoUrl, "utf8")).split(deploymentInfoMarker)[0];
const deploymentInfoAliases = ["Heartbeats", "Telemetry", "Updates"].flatMap(name => [
  `export { SetupUpdate${name} as DeploymentInfo${name} };`,
  `export { SetupUpdate${name}$inboundSchema as DeploymentInfo${name}$inboundSchema };`,
]);
await writeFile(deploymentInfoUrl, `${deploymentInfoSource}${deploymentInfoMarker}${deploymentInfoAliases.join("\n")}\n`);
