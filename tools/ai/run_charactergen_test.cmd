@echo off
rem Tries CharacterGen on the example picture (a test, run by hand). The first run compiles nvdiffrast for the graphics card with the Visual Studio 2022
rem build tools and downloads Stable Diffusion 2.1 and a background remover: it takes a while. The answer is in the log next to this file.
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >nul
set TORCH_CUDA_ARCH_LIST=8.6
set DISTUTILS_USE_SDK=1
set MSSdk=1
"C:\Users\SaphiresSilver\SeviathAI\CG-venv\Scripts\python.exe" "C:\Users\SaphiresSilver\Projects\seviathstudio-rs\tools\ai\shape_charactergen.py" --image "C:\Users\SaphiresSilver\Projects\seviathstudio-rs\target\gen\c2\gen_3.png" --out "C:\Users\SaphiresSilver\Projects\seviathstudio-rs\target\gen\cg_shape.glb" --repo "C:\Users\SaphiresSilver\SeviathAI\CharacterGen-code" --views-out "C:\Users\SaphiresSilver\Projects\seviathstudio-rs\target\gen\cg_views" > "C:\Users\SaphiresSilver\Projects\seviathstudio-rs\target\gen\cg_run.log" 2>&1
echo done, see target\gen\cg_run.log
