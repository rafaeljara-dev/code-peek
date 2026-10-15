"""Inventario reproducible del lockfile y textos de licencias de la caché Cargo."""
import argparse
import hashlib
import json
import pathlib
import subprocess

ROOT = pathlib.Path(__file__).resolve().parent.parent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--allow-network", action="store_true")
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    command = ["cargo", "metadata", "--locked", "--format-version=1"]
    if not args.allow_network:
        command.append("--offline")
    metadata = json.loads(subprocess.check_output(command, cwd=ROOT))
    packages = sorted((p for p in metadata["packages"] if p["source"]), key=lambda p: (p["name"], p["version"]))
    texts = {}
    rows = []
    for package in packages:
        directory = pathlib.Path(package["manifest_path"]).parent
        candidates = []
        for item in sorted(directory.iterdir()):
            if item.is_symlink():
                continue
            if item.name.lower().startswith(("license", "licence", "copying", "copyright", "notice")):
                if item.is_file():
                    candidates.append(item)
                elif item.is_dir():
                    candidates.extend(p for p in sorted(item.iterdir()) if p.is_file() and not p.is_symlink())
        references = []
        for file in candidates:
            if file.stat().st_size > 256 * 1024:
                raise RuntimeError(f"Aviso excesivo: {package['name']}/{file.name}")
            content = file.read_text(encoding="utf-8", errors="strict").replace("\r\n", "\n").strip()
            digest = hashlib.sha256(content.encode()).hexdigest()[:16]
            texts.setdefault(digest, content)
            references.append(f"[{file.relative_to(directory).as_posix()}](#license-{digest})")
        license_id = package["license"] or "Revisión manual requerida"
        rows.append(f"| {package['name']} {package['version']} | {license_id} | {', '.join(references) or 'Consultar fuente de la dependencia'} |")
    header = """# Avisos de terceros de CodePeek

El código propio se distribuye bajo MIT (LICENSE). Las dependencias y fuentes
conservan sus licencias y derechos. Para Slint distribuido con esta aplicación de
escritorio se elige Slint Royalty-free 2.0; la atribución AboutSlint está disponible
en «Acerca de». Esto no relicencia Slint bajo MIT. Un fork debe conservar avisos y
atribución y revisar las condiciones al cambiar plataforma o dependencias.

Este inventario es un **superset del lockfile**, incluye herramientas de build,
pruebas y otras plataformas: no afirma que todo esté enlazado al ejecutable.
Los textos se deduplican por SHA-256. No sustituye una revisión de compatibilidad.
Regenerar con `python scripts/generate-notices.py` después de actualizar Cargo.lock.

Devicon: Copyright (c) 2015 konpa, MIT. Codicons: Microsoft, CC BY 4.0.
Se recortaron las fuentes a los glifos usados; los archivos originales conservan
su atribución. Véanse DEVICON-LICENSE, CODICONS-LICENSE y crates/peek-ui/assets/README.md.

## Dependencias

| Paquete | Licencia declarada | Textos suministrados por el paquete |
| --- | --- | --- |
"""
    output = header + "\n".join(rows) + "\n\n## Textos de licencias\n\n"
    for digest, content in sorted(texts.items()):
        output += f'<a id="license-{digest}"></a>\n\n### license-{digest}\n\n'
        output += "\n".join(("> " + line).rstrip() for line in content.splitlines()) + "\n\n"
    output = output.rstrip() + "\n"
    target = ROOT / "THIRD-PARTY-NOTICES.md"
    if args.check:
        if not target.exists() or target.read_text(encoding="utf-8") != output:
            raise SystemExit("THIRD-PARTY-NOTICES.md requiere regeneración.")
    else:
        target.write_text(output, encoding="utf-8", newline="\n")
    print(f"Avisos: {len(packages)} paquetes, {len(texts)} textos únicos.")


if __name__ == "__main__":
    main()
