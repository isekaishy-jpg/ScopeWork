"""Manual scale qualification; creates isolated synthetic research only."""
import argparse, json, pathlib, subprocess, time
parser=argparse.ArgumentParser()
parser.add_argument('--exe',required=True);parser.add_argument('--name',required=True)
parser.add_argument('--count',type=int,default=150000)
parser.add_argument('--probes',action='store_true')
args=parser.parse_args()
root=pathlib.Path(__file__).resolve().parents[1]
home=root/'target'/'qualification'/args.name
home.mkdir(parents=True,exist_ok=False)
exe=pathlib.Path(args.exe).resolve()
def run(*cmd):
    p=subprocess.run([str(exe),'--home',str(home),*map(str,cmd)],capture_output=True,text=True)
    if p.returncode: raise RuntimeError(p.stderr)
    return json.loads(p.stdout)
run('project','create','scale','--name','Scale','--binary','synthetic-scale-12340')
run('node','create','scale','--parent',0,'--name','Existing architecture')
run('node','create','scale','--parent',1,'--name','Existing chunk')
run('node','create','scale','--parent',2,'--name','Editable existing function','--rva','1000')
p=run('project','show','scale')
create=[];nid=p['next_id'];chunks=[]
for a in range(5):
    arch=nid;nid+=1;create.append(dict(parent=0,name=f'Architecture {a}'))
    for c in range(30):
        chunks.append(nid);nid+=1;create.append(dict(parent=arch,name=f'Chunk {a}-{c}'))
for i in range(args.count):create.append(dict(parent=chunks[i%len(chunks)],name=f'Function {i}',rva=0x10000+i*16))
request=home/'request.json';request.write_text(json.dumps(dict(schema=1,binary=p['binary'],expect_revision=p['revision'],create=create)))
start=time.perf_counter()
with (home/'stdout.json').open('w') as out,(home/'stderr.log').open('w') as err:
    process=subprocess.Popen([str(exe),'--home',str(home),'node','batch','scale','--file',str(request)],stdout=out,stderr=err)
    probes=[]
    if args.probes:
        time.sleep(1)
        for i in range(20):
            if process.poll() is not None:break
            try:
                read_start=time.perf_counter();record=run('node','show','scale',3);read_ms=(time.perf_counter()-read_start)*1000
                body=home/'probe-body.md';evidence=home/'probe-evidence.md';body.write_text(f'Synthetic concurrent edit {i}');evidence.write_text('Synthetic evidence fixture')
                save_start=time.perf_counter();run('node','save','scale',3,'--body-file',body,'--evidence-file',evidence,'--expect',record['token']);save_ms=(time.perf_counter()-save_start)*1000
                probes.append(dict(read_ms=read_ms,save_ms=save_ms))
            except RuntimeError as error:probes.append(dict(error=str(error)))
            (home/'probes.json').write_text(json.dumps(probes,indent=2));time.sleep(2)
    result=process.wait()
summary=dict(probes=probes,executable=str(exe),functions=args.count,new_records=len(create),seconds=time.perf_counter()-start,exit_code=result,home=str(home))
(home/'measurement.json').write_text(json.dumps(summary,indent=2))
print(json.dumps(summary),flush=True)
