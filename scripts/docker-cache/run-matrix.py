#!/usr/bin/env python3
"""Synthetic end-to-end cache, invalidation, transport, and runtime checks."""
import concurrent.futures, hashlib, json, os, pathlib, shutil, subprocess, time
import argparse
parser=argparse.ArgumentParser(description='Run synthetic Docker cache experiments in a disposable Docker environment.')
parser.add_argument('--output',type=pathlib.Path,required=True)
parser.add_argument('--push-probe',type=pathlib.Path,required=True)
parser.add_argument('--cache-url',required=True)
parser.add_argument('--cache-log',type=pathlib.Path,required=True)
parser.add_argument('--registry-log',type=pathlib.Path,required=True)
parser.add_argument('--registry-host',default='127.0.0.1:51616')
parser.add_argument('--prefix',default='cache-experiment')
args=parser.parse_args()
root=args.output.resolve();root.mkdir(parents=True,exist_ok=False)
repo=pathlib.Path(__file__).resolve().parents[2]
probe=repo/'target/debug/examples/docker-cache-probe'
push=args.push_probe.resolve()
wrapper=root/'bin';wrapper.mkdir();(wrapper/'docker').symlink_to(repo/'scripts/docker-cache/docker-wrapper')
base={**os.environ,'REAL_DOCKER':shutil.which('docker'),'PATH':str(wrapper)+':'+os.environ['PATH']}
remote={'PROBE_CACHE_URL':args.cache_url}
results={}
source=root/'matrix-source'
shutil.copytree(repo/'scripts/docker-cache',source,dirs_exist_ok=True)

def command(label,args,env=base,expected=0):
    start=time.time()
    with open(root/(label+'.log'),'w') as output:
        result=subprocess.run([str(a) for a in args],env=env,stdout=output,stderr=subprocess.STDOUT)
    with open(root/'commands.jsonl','a') as output:
        output.write(json.dumps({'label':label,'argv':[str(a) for a in args],'exitStatus':result.returncode,'seconds':time.time()-start})+'\n')
    assert result.returncode==expected,(label,result.returncode)
    return time.time()-start

def traffic(name):
    p=args.cache_log if name=='cache' else args.registry_log
    return [json.loads(line) for line in p.read_text().splitlines()] if p.exists() else []

def total(entries):
    entries=list(entries)
    return {key:sum(e[key] for e in entries) for key in ['requestBytes','responseBytes']}

def build(label,env,src=source,exe=probe,expected=0):
    expected_inputs={'app.txt':(src/'app.txt').read_text().strip(), 'dependency.txt':(src/'dependency.txt').read_text().strip(), 'dependencies/version':env.get('PROBE_DEP_VERSION','1')}
    count=len(traffic('cache'))
    seconds=command(label,[exe,src,root/label],env,expected)
    entry={'seconds':seconds,'exitStatus':expected,'cacheTraffic':total(traffic('cache')[count:])}
    if expected:
        results[label]=entry
        return entry
    archive=root/label/'linux-x64.oci.tar'
    inspect=[ 'python3',repo/'scripts/docker-cache/inspect-image.py',archive,'--before',str(archive)+'.before-normalization']
    command(label+'-inspect',inspect)
    data=json.loads((root/(label+'-inspect.log')).read_text())
    for path, expected_value in expected_inputs.items():
        assert data['files'][path]==expected_value,(label,path,data['files'][path],expected_value)
    entry['expectedInputs']=expected_inputs
    entry['image']=data
    count=len(traffic('registry'))
    command(label+'-push',[push,archive,args.registry_host+'/synthetic/matrix:'+label])
    entry['pushTraffic']=total(e for e in traffic('registry')[count:] if e['path'].startswith('/v2/synthetic/matrix/'))
    command(label+'-registry-audit',['python3',repo/'scripts/docker-cache/inspect-image.py',archive,'--registry','http://'+args.registry_host+'/v2/synthetic/matrix/manifests/'+label])
    command(label+'-load',[base['REAL_DOCKER'],'load','-i',archive])
    command(label+'-runtime',[base['REAL_DOCKER'],'run','--rm','--read-only','--network','none',data['config']['digest']])
    expected_output='\n'.join([expected_inputs['app.txt'],expected_inputs['dependency.txt'],expected_inputs['dependencies/version'],'/dependencies/table: OK'])+'\n'
    assert (root/(label+'-runtime.log')).read_text()==expected_output
    results[label]=entry
    (root/'matrix-results.json').write_text(json.dumps(results,indent=2))
    print(label,round(seconds,3),entry['cacheTraffic'],entry['pushTraffic'],flush=True)
    return entry

def fresh(label):
    name=args.prefix+'-'+label
    command(label+'-create',[base['REAL_DOCKER'],'buildx','create','--name',name,'--driver','docker-container'])
    return {**base,**remote,'BUILDX_BUILDER':name}

def same_dep(a,b):
    assert a['image']['files']['dependencies/build-nonce']==b['image']['files']['dependencies/build-nonce']
    assert a['image']['layers'][2]==b['image']['layers'][2]

def new_dep(a,b):
    assert a['image']['files']['dependencies/build-nonce']!=b['image']['files']['dependencies/build-nonce']
    assert a['image']['layers'][2]['digest']!=b['image']['layers'][2]['digest']

local={**base,'BUILDX_BUILDER':'default'}
# For a true cold local result use a fresh/disposable daemon. Never prune an
# existing user's cache here; the first local result may otherwise already be warm.
a=build('matrix-local-cold',local)
b=build('matrix-local-warm',local);same_dep(a,b)
a=build('matrix-remote-cold',fresh('matrix-remote-cold'))
command('remote-cold-remove',[base['REAL_DOCKER'],'buildx','rm',args.prefix+'-matrix-remote-cold'])
b=build('matrix-remote-warm',fresh('matrix-remote-warm'));same_dep(a,b)
(source/'app.txt').write_text('application-v2\n')
c=build('matrix-app-change',fresh('matrix-app-change'));same_dep(b,c)
assert b['image']['layers'][3]['digest']!=c['image']['layers'][3]['digest']
(source/'dependency.txt').write_text('dependency-v2\n')
d=build('matrix-dependency-change',fresh('matrix-dependency-change'));new_dep(c,d)
e=build('matrix-arg-change',{**fresh('matrix-arg-change'),'PROBE_DEP_VERSION':'2'});new_dep(d,e)
# Dockerfile change affects its dependency step, not only image metadata.
dockerfile=(source/'Dockerfile').read_text()
(source/'Dockerfile').write_text(dockerfile.replace('300000','300001'))
f=build('matrix-dockerfile-change',{**fresh('matrix-dockerfile-change'),'PROBE_DEP_VERSION':'2'});new_dep(e,f)
# Failed builds must not overwrite the last successful cache snapshot.
(source/'Dockerfile').write_text((source/'Dockerfile').read_text()+'\nRUN exit 19\n')
failed_env={**fresh('matrix-failure'),'PROBE_DEP_VERSION':'2'}
failed=build('matrix-failure',failed_env,expected=1)
assert failed['cacheTraffic']['requestBytes']==0
(source/'Dockerfile').write_text(dockerfile.replace('300000','300001'))
g=build('matrix-recovery',failed_env);same_dep(f,g)
# Two fresh builders import and publish concurrently; both distinct apps must run.
sources=[]
for i in range(2):
    path=root/('concurrent-source-'+str(i));shutil.copytree(source,path,dirs_exist_ok=True)
    (path/'app.txt').write_text('concurrent-'+str(i)+'\n');sources.append(path)
envs=[{**fresh('matrix-concurrent-'+str(i)),'PROBE_DEP_VERSION':'2'} for i in range(2)]
# Record build wall times concurrently, audit sequentially to keep push counters exact.
with concurrent.futures.ThreadPoolExecutor(2) as executor:
    futures=[executor.submit(command,'matrix-concurrent-'+str(i),[probe,sources[i],root/('matrix-concurrent-'+str(i))],envs[i]) for i in range(2)]
    for future in futures: future.result()
for i in range(2):
    label='matrix-concurrent-'+str(i)
    archive=root/label/'linux-x64.oci.tar'
    command(label+'-inspect',['python3',repo/'scripts/docker-cache/inspect-image.py',archive,'--before',str(archive)+'.before-normalization'])
    data=json.loads((root/(label+'-inspect.log')).read_text())
    assert data['files']['app.txt']==(sources[i]/'app.txt').read_text().strip()
    assert data['files']['dependency.txt']==(sources[i]/'dependency.txt').read_text().strip()
    assert data['files']['dependencies/version']=='2'
    command(label+'-load',[base['REAL_DOCKER'],'load','-i',archive])
    command(label+'-runtime',[base['REAL_DOCKER'],'run','--rm','--read-only','--network','none',data['config']['digest']])
    assert (root/(label+'-runtime.log')).read_text()=='concurrent-'+str(i)+'\ndependency-v2\n2\n/dependencies/table: OK\n'
# Reuse the resulting snapshots from another genuinely fresh builder and verify output.
h=build('matrix-after-concurrency',{**fresh('matrix-after-concurrency'),'PROBE_DEP_VERSION':'2'});same_dep(g,h)
(root/'matrix-results.json').write_text(json.dumps(results,indent=2))
