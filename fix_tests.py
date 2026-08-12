with open('src/cmd/clean_tests.rs', 'r') as f:
    lines = f.readlines()
with open('src/cmd/clean_tests.rs', 'w') as f:
    f.writelines(lines[2:-1])
