"""Small import smoke measurement. Fixture cleanup is mandatory, including on failure."""
import argparse
import json
import pathlib
import re
import subprocess
import tempfile
import time


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--exe', required=True)
    parser.add_argument('--name', required=True)
    parser.add_argument('--count', type=int, default=16)
    parser.add_argument('--probes', action='store_true')
    args = parser.parse_args()
    if not 1 <= args.count <= 100:
        parser.error('--count must be between 1 and 100; large on-disk fixtures are prohibited')
    if not re.fullmatch(r'[a-zA-Z0-9_-]{1,64}', args.name):
        parser.error('--name must be 1-64 letters, digits, underscores or hyphens')
    exe = pathlib.Path(args.exe).resolve(strict=True)
    output = pathlib.Path(__file__).resolve().parents[1] / 'target' / 'qualification'
    output.mkdir(parents=True, exist_ok=True)
    summary_path = output / (args.name + '.json')
    with summary_path.open('x', encoding='utf-8') as summary_file:
        with tempfile.TemporaryDirectory(prefix='Scopework-import-smoke-') as scratch:
            home = pathlib.Path(scratch)

            def run(*cmd):
                p = subprocess.run([str(exe), '--home', str(home), *map(str, cmd)],
                                   capture_output=True, text=True, timeout=120)
                if p.returncode:
                    raise RuntimeError(p.stderr)
                return json.loads(p.stdout)

            run('project', 'create', 'scale', '--name', 'Synthetic smoke', '--binary', 'synthetic-smoke')
            run('node', 'create', 'scale', '--parent', 0, '--name', 'Architecture')
            run('node', 'create', 'scale', '--parent', 1, '--name', 'Chunk')
            run('node', 'create', 'scale', '--parent', 2, '--name', 'Existing function', '--rva', '1000')
            project = run('project', 'show', 'scale')
            create = [dict(parent=2, name=f'Function {i}', rva=0x10000+i*16) for i in range(args.count)]
            request = home / 'request.json'
            request.write_text(json.dumps(dict(schema=1, binary=project['binary'],
                                               expect_revision=project['revision'], create=create)), encoding='utf-8')
            probes = []
            start = time.perf_counter()
            with (home / 'stdout.json').open('w') as out, (home / 'stderr.log').open('w') as err:
                process = subprocess.Popen([str(exe), '--home', str(home), 'node', 'batch', 'scale',
                                            '--file', str(request)], stdout=out, stderr=err)
                try:
                    if args.probes:
                        for _ in range(3):
                            if process.poll() is not None:
                                break
                            read_start = time.perf_counter()
                            try:
                                run('node', 'show', 'scale', 3)
                                probes.append(dict(read_ms=(time.perf_counter()-read_start)*1000))
                            except RuntimeError as error:
                                probes.append(dict(error=str(error)))
                    result = process.wait(timeout=120)
                finally:
                    if process.poll() is None:
                        process.kill()
                        process.wait()
            summary = dict(probes=probes, executable=str(exe), functions=args.count,
                           seconds=time.perf_counter()-start, exit_code=result,
                           stdout=(home / 'stdout.json').read_text(), stderr=(home / 'stderr.log').read_text())
            json.dump(summary, summary_file, indent=2)
        print(json.dumps(dict(result=str(summary_path), fixture_removed=True, exit_code=result)))
        if result:
            raise SystemExit(result)


if __name__ == '__main__':
    main()
