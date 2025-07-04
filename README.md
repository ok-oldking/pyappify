# PyAppify

A modern Python packaging tool to create distributable applications with built-in updating, inspired by `uv`. It solves many common drawbacks found in tools like PyInstaller and Nuitka.
![img.png](readme/img_1.png)
## Features

*   **Lightweight Launcher**: The GUI launcher is built in Rust and is only ~3MB zipped.
*   **Dynamic Setup**: On first launch, the application clones your code from a Git repository, downloads an isolated Python environment, and installs dependencies using `pip`.
*   **Universal Compatibility**: Supports all Python libraries without special configuration.
*   **Robust App Management**: The GUI provides industry-level upgrading and downgrading backed by Git and `pip`, with auto-generated update notes.
*   **Blazing-Fast Updates**: Typical incremental take about one second.
*   **Multiple Profiles**: Define and allow users to switch between different application profiles (e.g., CPU vs. CUDA versions) with unique entry points and dependencies.
*   **CI/CD Integration**: A dedicated GitHub Action can pre-build a full package with all dependencies included for offline distribution.

## Quick Start: Prebuilt Launcher

This method distributes the lightweight `PyAppify` launcher. The launcher will then download your application and its dependencies on the user's machine.

1.  Download the latest `pyappify` executable from the project's [Releases page](https://github.com/ok-oldking/pyappify/releases).
2.  Modify the `pyappify.yml` file according to your python project.

```yaml
# pyappify.yml
name: "pyappify-sample" # English only
profiles:
  - name: "release_正式版" # can use Unicode chars
    git_url: "https://github.com/ok-oldking/pyappify-action.git" # The repo url to clone. Must have tags for version management; semver is recommended.
    main_script: "main.py" # If ending with .py, will use python venv to run. Otherwise, will search in the working dir and the venv's Script/bin path.
    requires_python: "3.12" # Supports python 3.7 - 3.13.
    requirements: "requirements.txt"  # Supports a requirements.txt file or pyproject.toml like .[dev,docs].
    pip_args: "--index-url https://mirrors.cloud.tencent.com/pypi/simple" # Optional pip arguments.

  - name: "debug" # Optional Another profile.
    main_script: "main_debug.py" # You can omit other properties; they will default to the values from the first profile.
    pip_args: "-i https://mirrors.aliyun.com/pypi/simple" # Optional pip arguments.
```

3. You can test the launcher by double-clicking the pyappify.exe and install python with the GUI. You can then package the files for offline or online distribution.

* pyappify.yml (Required, You project config.)
* pyappify.exe (You can rename it to your app name.)
* data (python, venv, dependencies, git repo, include if you want the offline full package.)
* logs(pyappify logs and your console log, auto rotate, can be deleted.)
* cache(pip cache etc, can be deleted.)


## Quick Start: Pre-packaged Release with GitHub Actions

This method uses the PyAppify GitHub Action to bundle the launcher, your code, and all dependencies into a single distributable zip file. This is ideal for users who need a fully offline installation.

### Prerequisites

1.  A `pyappify.yml` file in the root of your repository, configured for your project.
2.  (Optional) An `icons` directory in your repository root containing `icon.ico` and `icon.png` for custom application icons.

### Workflow Example

refer to the docs at [https://github.com/ok-oldking/pyappify-action](https://github.com/ok-oldking/pyappify-action)