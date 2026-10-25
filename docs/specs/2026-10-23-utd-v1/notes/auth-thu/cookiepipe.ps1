# THỬ (prototype, không phải code UTD): named pipe đóng vai file cookies.txt cho yt-dlp.
# Pipe chỉ cho SID người dùng hiện tại, từ chối client từ xa, mỗi lần nối: ghi nội dung cookie rồi
# đọc những gì client ghi lại (yt-dlp lưu cookie lúc thoát), ghi nhật ký TÊN cookie, không ghi giá trị.
param(
  [Parameter(Mandatory = $true)][string]$Name,
  [Parameter(Mandatory = $true)][string]$CookieFile,
  [int]$Connections = 4,
  [string]$Log,
  [string]$Mode = "InOut"
)

Add-Type -TypeDefinition @"
using System;
using System.IO;
using System.IO.Pipes;
using System.Runtime.InteropServices;
using System.Security.AccessControl;
using System.Security.Principal;
using System.Text;
using System.Threading;
using System.Threading.Tasks;

public static class CookiePipe {
  [DllImport("kernel32.dll", SetLastError = true)]
  static extern bool GetNamedPipeClientProcessId(Microsoft.Win32.SafeHandles.SafePipeHandle pipe, out uint pid);

  static string Names(string text) {
    var sb = new StringBuilder();
    foreach (var line in text.Split('\n')) {
      var l = line.Trim();
      if (l.Length == 0 || (l.StartsWith("#") && !l.StartsWith("#HttpOnly_"))) continue;
      var parts = l.Split('\t');
      if (parts.Length >= 7) sb.Append(parts[0]).Append(':').Append(parts[5]).Append(' ');
    }
    return sb.ToString();
  }

  public static void Serve(string name, byte[] payload, int connections, string log, bool outOnly) {
    var security = new PipeSecurity();
    var me = WindowsIdentity.GetCurrent().User;
    security.AddAccessRule(new PipeAccessRule(me, PipeAccessRights.FullControl, AccessControlType.Allow));
    // PipeOptions 0x20000000 không có trong .NET Framework; dùng WriteThrough=0x80000000? -> không. Chỉ Asynchronous.
    // Tạo sẵn đủ số instance: yt-dlp mở pipe liên tiếp (os.access rồi open), instance bận => EINVAL.
    var servers = new NamedPipeServerStream[connections];
    for (int k = 0; k < connections; k++)
      servers[k] = new NamedPipeServerStream(name, outOnly ? PipeDirection.Out : PipeDirection.InOut, connections, PipeTransmissionMode.Byte,
                                             PipeOptions.Asynchronous, 65536, 65536, security);
    var tasks = new Task[connections];
    var order = 0;
    for (int k = 0; k < connections; k++) {
      var server = servers[k];
      tasks[k] = Task.Run(() => {
        var wait = server.WaitForConnectionAsync();
        if (!wait.Wait(TimeSpan.FromSeconds(60))) { lock (log) File.AppendAllText(log, "no client for an instance\n"); return; }
        var i = Interlocked.Increment(ref order);
        var t0 = DateTime.Now.ToString("HH:mm:ss.fff");
        uint pid; GetNamedPipeClientProcessId(server.SafePipeHandle, out pid);
        string proc = "?";
        try { proc = System.Diagnostics.Process.GetProcessById((int)pid).ProcessName; } catch {}
        var sent = "no";
        var received = new MemoryStream();
        try {
          var w = server.WriteAsync(payload, 0, payload.Length);
          if (w.Wait(1500)) sent = "yes";
          if (outOnly) {
            // Chỉ đọc: chờ client đọc hết rồi ĐÓNG (không Disconnect: client nhận EOF thay vì lỗi).
            var drain = Task.Run(() => server.WaitForPipeDrain());
            sent += drain.Wait(3000) ? "+drained" : "+nodrain";
          } else {
            // Client đọc (load) hay ghi (save)? Đua: drain xong = client đọc; đọc được byte = client ghi.
            var buf = new byte[8192];
            var drain = Task.Run(() => server.WaitForPipeDrain());
            var read = server.ReadAsync(buf, 0, buf.Length);
            var first = Task.WaitAny(new Task[] { drain, read }, 5000);
            if (first == 0) sent += "+drained";
            else if (first == 1 && read.Result > 0) {
              received.Write(buf, 0, read.Result);
              while (true) {
                var r = server.ReadAsync(buf, 0, buf.Length);
                if (!r.Wait(3000) || r.Result == 0) break;
                received.Write(buf, 0, r.Result);
              }
            }
          }
        } catch (Exception e) { sent += "+err:" + e.GetType().Name; }
        var text = Encoding.UTF8.GetString(received.ToArray());
        lock (log) File.AppendAllText(log, String.Format("conn {0} @{6}: pid={1} proc={2} sent={3} gotBytes={4} gotCookies=[{5}]\n",
          i, pid, proc, sent, received.Length, Names(text), t0));
        server.Dispose();
      });
    }
    Task.WaitAll(tasks, TimeSpan.FromSeconds(90));
  }
}
"@

$payload = [System.IO.File]::ReadAllBytes($CookieFile)
[CookiePipe]::Serve($Name, $payload, $Connections, $Log, $Mode -eq "Out")
