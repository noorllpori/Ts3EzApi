@echo off
echo 正在查找 Nginx 进程...
tasklist /FI "IMAGENAME eq nginx.exe"

echo 正在终止 Nginx 进程...
taskkill /IM nginx.exe /F
if %errorlevel%==0 (
	echo Nginx 进程已成功终止。
	
pause