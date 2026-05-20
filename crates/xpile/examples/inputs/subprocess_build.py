def build() -> int:
    subprocess.run(["echo", "starting"])
    subprocess.run(["ls", "/tmp"])
    subprocess.run(["pwd"])
    subprocess.run(["echo", "done"])
    return 0
