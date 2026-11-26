param(
    [string]$PaintNetExe = "C:\Program Files\Paint.NET\paintdotnet.exe",
    [string]$FixturesDir = (Join-Path $PSScriptRoot "..\tests\fixtures\pdn")
)

$ErrorActionPreference = "Stop"

Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

Add-Type -ReferencedAssemblies @("UIAutomationClient", "UIAutomationTypes") -TypeDefinition @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public class WInfo {
    public IntPtr Hwnd;
    public string Title;
    public string ClassName;
    public int CtrlId;
}

[StructLayout(LayoutKind.Sequential)]
public struct WINRECT {
    public int Left, Top, Right, Bottom;
}

[StructLayout(LayoutKind.Sequential)]
public struct COMBOBOXINFO {
    public int cbSize;
    public WINRECT rcItem;
    public WINRECT rcButton;
    public int stateButton;
    public IntPtr hwndCombo;
    public IntPtr hwndEdit;
    public IntPtr hwndList;
}

public static class PdnWin32 {
    public delegate bool EnumProc(IntPtr h, IntPtr lp);

    [DllImport("user32.dll")]
    public static extern bool EnumWindows(EnumProc f, IntPtr lp);

    [DllImport("user32.dll")]
    public static extern bool EnumChildWindows(IntPtr p, EnumProc f, IntPtr lp);

    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);

    [DllImport("user32.dll")]
    public static extern bool IsWindowVisible(IntPtr h);

    [DllImport("user32.dll")]
    public static extern int GetDlgCtrlID(IntPtr h);

    [DllImport("user32.dll")]
    public static extern IntPtr GetParent(IntPtr h);

    [DllImport("user32.dll")]
    public static extern bool GetWindowRect(IntPtr h, out WINRECT r);

    [DllImport("user32.dll")]
    public static extern bool GetComboBoxInfo(IntPtr hwndCombo, ref COMBOBOXINFO pcbi);

    [DllImport("user32.dll")]
    public static extern bool PostMessage(IntPtr h, uint msg, IntPtr wp, IntPtr lp);

    [DllImport("user32.dll")]
    public static extern IntPtr SendMessage(IntPtr h, uint msg, IntPtr wp, IntPtr lp);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern IntPtr SendMessage(IntPtr h, uint msg, IntPtr wp, string lp);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern IntPtr SendMessage(IntPtr h, uint msg, IntPtr wp, StringBuilder lp);

    [DllImport("user32.dll")]
    public static extern IntPtr SendMessage(IntPtr h, uint msg, IntPtr wp, ref WINRECT lp);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetWindowText(IntPtr h, StringBuilder sb, int max);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetClassName(IntPtr h, StringBuilder sb, int max);

    public static List<WInfo> Top(int pid) {
        var list = new List<WInfo>();
        EnumWindows((h, l) => {
            uint p;
            GetWindowThreadProcessId(h, out p);
            if (p == (uint)pid && IsWindowVisible(h)) {
                var t = new StringBuilder(512);
                var c = new StringBuilder(256);
                GetWindowText(h, t, t.Capacity);
                GetClassName(h, c, c.Capacity);
                list.Add(new WInfo { Hwnd = h, Title = t.ToString(), ClassName = c.ToString() });
            }
            return true;
        }, IntPtr.Zero);
        return list;
    }

    public static List<WInfo> Children(IntPtr parent) {
        var list = new List<WInfo>();
        EnumChildWindows(parent, (h, l) => {
            var t = new StringBuilder(512);
            var c = new StringBuilder(256);
            GetWindowText(h, t, t.Capacity);
            GetClassName(h, c, c.Capacity);
            list.Add(new WInfo { Hwnd = h, Title = t.ToString(), ClassName = c.ToString(), CtrlId = GetDlgCtrlID(h) });
            return true;
        }, IntPtr.Zero);
        return list;
    }

    public static void ClickScreenPointInWindow(IntPtr hwnd, int screenX, int screenY) {
        WINRECT r;
        GetWindowRect(hwnd, out r);
        int lx = screenX - r.Left;
        int ly = screenY - r.Top;
        IntPtr lp = (IntPtr)((ly << 16) | (lx & 0xFFFF));
        PostMessage(hwnd, 0x0200, IntPtr.Zero, lp); // WM_MOUSEMOVE
        PostMessage(hwnd, 0x0201, (IntPtr)1, lp);   // WM_LBUTTONDOWN
        PostMessage(hwnd, 0x0202, IntPtr.Zero, lp); // WM_LBUTTONUP
    }

    public static string SelectComboItemByRegex(IntPtr dialogHwnd, string pattern) {
        var rx = new System.Text.RegularExpressions.Regex(pattern, System.Text.RegularExpressions.RegexOptions.IgnoreCase);
        foreach (var ch in Children(dialogHwnd)) {
            if (!ch.ClassName.Contains("ComboBox")) continue;
            int count = (int)SendMessage(ch.Hwnd, 0x0146, IntPtr.Zero, IntPtr.Zero); // CB_GETCOUNT
            for (int i = 0; i < count; i++) {
                var sb = new StringBuilder(256);
                SendMessage(ch.Hwnd, 0x0148, (IntPtr)i, sb); // CB_GETLBTEXT
                string itemText = sb.ToString();
                if (!rx.IsMatch(itemText)) continue;

                var cbi = new COMBOBOXINFO();
                cbi.cbSize = Marshal.SizeOf(typeof(COMBOBOXINFO));
                SendMessage(ch.Hwnd, 0x014F, (IntPtr)1, IntPtr.Zero); // CB_SHOWDROPDOWN(true)
                System.Threading.Thread.Sleep(150);
                if (GetComboBoxInfo(ch.Hwnd, ref cbi) && cbi.hwndList != IntPtr.Zero) {
                    WINRECT itemRect = new WINRECT();
                    SendMessage(cbi.hwndList, 0x0198, (IntPtr)i, ref itemRect); // LB_GETITEMRECT
                    int cx = (itemRect.Left + itemRect.Right) / 2;
                    int cy = (itemRect.Top + itemRect.Bottom) / 2;
                    IntPtr lp = (IntPtr)((cy << 16) | (cx & 0xFFFF));
                    PostMessage(cbi.hwndList, 0x0200, IntPtr.Zero, lp);
                    PostMessage(cbi.hwndList, 0x0201, (IntPtr)1, lp);
                    PostMessage(cbi.hwndList, 0x0202, IntPtr.Zero, lp);
                } else {
                    SendMessage(ch.Hwnd, 0x014E, (IntPtr)i, IntPtr.Zero); // CB_SETCURSEL
                    IntPtr parent = GetParent(ch.Hwnd);
                    int ctrlId = GetDlgCtrlID(ch.Hwnd) & 0xFFFF;
                    SendMessage(parent, 0x0111, (IntPtr)((1 << 16) | ctrlId), ch.Hwnd); // CBN_SELCHANGE
                    SendMessage(parent, 0x0111, (IntPtr)((9 << 16) | ctrlId), ch.Hwnd); // CBN_SELENDOK
                    SendMessage(ch.Hwnd, 0x014F, IntPtr.Zero, IntPtr.Zero);
                }
                return itemText;
            }
        }
        return null;
    }

    public static void ClickButton(IntPtr btnHwnd) {
        WINRECT r;
        GetWindowRect(btnHwnd, out r);
        int lx = (r.Right - r.Left) / 2;
        int ly = (r.Bottom - r.Top) / 2;
        IntPtr lp = (IntPtr)((ly << 16) | (lx & 0xFFFF));
        PostMessage(btnHwnd, 0x0200, IntPtr.Zero, lp);
        PostMessage(btnHwnd, 0x0201, (IntPtr)1, lp);
        PostMessage(btnHwnd, 0x0202, IntPtr.Zero, lp);
    }

    public static void CloseWindow(IntPtr hwnd) {
        PostMessage(hwnd, 0x0010, IntPtr.Zero, IntPtr.Zero); // WM_CLOSE
    }

    public static void SetText(IntPtr editHwnd, string text) {
        SendMessage(editHwnd, 0x000C, IntPtr.Zero, ""); // clear
        SendMessage(editHwnd, 0x00B1, IntPtr.Zero, (IntPtr)(-1)); // EM_SETSEL(0, -1)
        SendMessage(editHwnd, 0x00C2, (IntPtr)1, text); // EM_REPLACESEL
        IntPtr parent = GetParent(editHwnd);
        int id = GetDlgCtrlID(editHwnd) & 0xFFFF;
        SendMessage(parent, 0x0111, (IntPtr)((0x0300 << 16) | id), editHwnd); // EN_CHANGE
    }
}
"@

function Wait-Until {
    param(
        [scriptblock]$Condition,
        [int]$TimeoutMs = 20000,
        [int]$PollMs = 150,
        [string]$FailureMessage = "Timed out"
    )
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    while ($sw.ElapsedMilliseconds -lt $TimeoutMs) {
        $res = & $Condition
        if ($res) { return $res }
        Start-Sleep -Milliseconds $PollMs
    }
    throw $FailureMessage
}

function Open-SaveAsViaMenuClick {
    param(
        [IntPtr]$MainHwnd,
        [int]$ProcessId
    )
    $mainEl = [System.Windows.Automation.AutomationElement]::FromHandle($MainHwnd)
    $menuCond = New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
        [System.Windows.Automation.ControlType]::MenuItem
    )
    $menus = $mainEl.FindAll([System.Windows.Automation.TreeScope]::Descendants, $menuCond)
    $fileMenu = $null
    foreach ($m in $menus) {
        if ($m.Current.Name -match "^(File|Arquivo)$") {
            $fileMenu = $m
            break
        }
    }
    if (-not $fileMenu) {
        throw "Could not find File menu on Paint.NET MainForm."
    }

    $exp = $fileMenu.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern)
    $exp.Expand()

    $clickTarget = Wait-Until -TimeoutMs 5000 -PollMs 100 -FailureMessage "File -> Save As... menu item did not appear." -Condition {
        $wins = [PdnWin32]::Top($ProcessId)
        foreach ($w in $wins) {
            if ($w.Hwnd -eq $MainHwnd -or $w.ClassName -like "UAC*") { continue }
            $popupEl = [System.Windows.Automation.AutomationElement]::FromHandle($w.Hwnd)
            $items = $popupEl.FindAll([System.Windows.Automation.TreeScope]::Descendants, $menuCond)
            foreach ($it in $items) {
                if ($it.Current.Name -match "Save As|Salvar como") {
                    $r = $it.Current.BoundingRectangle
                    if ($r.Width -gt 0 -and $r.Height -gt 0) {
                        return @{
                            PopupHwnd = $w.Hwnd
                            X = [int]($r.Left + $r.Width / 2)
                            Y = [int]($r.Top + $r.Height / 2)
                        }
                    }
                }
            }
        }
        return $null
    }

    [PdnWin32]::ClickScreenPointInWindow($clickTarget.PopupHwnd, $clickTarget.X, $clickTarget.Y)
}

function Assert-MagicBytes {
    param([string]$Path, [string]$TargetFormat)
    $bytes = [System.IO.File]::ReadAllBytes($Path)
    if ($bytes.Length -lt 8) {
        throw "Output file '$Path' is too small ($($bytes.Length) bytes)."
    }
    if ($TargetFormat -eq "PNG") {
        if ($bytes[0] -ne 0x89 -or $bytes[1] -ne 0x50 -or $bytes[2] -ne 0x4E -or $bytes[3] -ne 0x47) {
            throw "Output file '$Path' does not have PNG signature."
        }
    } else {
        if ($bytes[0] -ne 0x50 -or $bytes[1] -ne 0x44 -or $bytes[2] -ne 0x4E -or $bytes[3] -ne 0x33) {
            throw "Output file '$Path' does not have PDN3 signature."
        }
    }
}

function Export-ViaPaintNet52 {
    param(
        [string]$InputPdn,
        [string]$OutputPath,
        [ValidateSet("PNG", "PDN")]
        [string]$TargetFormat
    )
    $fullInput = (Resolve-Path $InputPdn).Path
    $fullOutput = [System.IO.Path]::GetFullPath($OutputPath)
    $stem = [System.IO.Path]::GetFileNameWithoutExtension($fullInput)

    if (Test-Path $fullOutput) {
        Remove-Item -Force $fullOutput
    }

    $existing = Get-Process -Name "paintdotnet" -ErrorAction SilentlyContinue
    if ($existing) {
        throw "paintdotnet.exe is already running; close existing Paint.NET instances before automated validation."
    }

    Write-Host "Opening $fullInput in Paint.NET 5.2 -> $fullOutput ($TargetFormat)..."
    $proc = Start-Process -FilePath $PaintNetExe -ArgumentList "`"$fullInput`"" -PassThru
    $mainWin = $null
    $hadError = $false
    try {
        $mainWin = Wait-Until -TimeoutMs 25000 -PollMs 150 -FailureMessage "Paint.NET main window for '$stem' did not appear." -Condition {
            if ($proc.HasExited) { throw "paintdotnet.exe exited unexpectedly." }
            $wins = [PdnWin32]::Top($proc.Id)
            foreach ($w in $wins) {
                if ($w.Title -like "*$stem*") { return $w }
            }
            return $null
        }
        Write-Host ("  MainForm ready: '" + $mainWin.Title + "'")
        Start-Sleep -Milliseconds 400

        Open-SaveAsViaMenuClick -MainHwnd $mainWin.Hwnd -ProcessId $proc.Id

        $saveWin = Wait-Until -TimeoutMs 10000 -PollMs 150 -FailureMessage "Save As dialog did not open for '$stem'." -Condition {
            $wins = [PdnWin32]::Top($proc.Id)
            foreach ($w in $wins) {
                if ($w.ClassName -eq "#32770" -or $w.Title -match "Save As|Salvar como") {
                    return $w
                }
            }
            return $null
        }
        Write-Host ("  Save dialog detected: '" + $saveWin.Title + "'")
        Start-Sleep -Milliseconds 250

        $typePattern = if ($TargetFormat -eq "PNG") { "^PNG\b|\(\*\.png\)" } else { "^Paint\.NET\b|\(\*\.pdn\)" }
        $selectedType = [PdnWin32]::SelectComboItemByRegex($saveWin.Hwnd, $typePattern)
        if (-not $selectedType) {
            throw "Failed to select '$TargetFormat' file type in Save As dialog."
        }
        Write-Host ("  Selected file type: " + $selectedType)
        Start-Sleep -Milliseconds 250

        $editCtrl = [PdnWin32]::Children($saveWin.Hwnd) | Where-Object { $_.ClassName -eq "Edit" -and $_.CtrlId -eq 1001 } | Select-Object -First 1
        if (-not $editCtrl) {
            throw "Could not find filename Edit control (CtrlId=1001) in Save As dialog."
        }
        [PdnWin32]::SetText($editCtrl.Hwnd, $fullOutput)
        Start-Sleep -Milliseconds 150

        $saveBtn = [PdnWin32]::Children($saveWin.Hwnd) | Where-Object { $_.ClassName -eq "Button" -and $_.CtrlId -eq 1 } | Select-Object -First 1
        if (-not $saveBtn) {
            throw "Could not find Save button (CtrlId=1) in Save As dialog."
        }
        [PdnWin32]::ClickButton($saveBtn.Hwnd)

        $bitDepthSet = ($TargetFormat -ne "PNG")
        $panelTitles = @("Colors", "Layers", "History", "Tools", "Cores", "Camadas", "Histórico", "Ferramentas", "Save As", "Salvar como")
        $swConfig = [System.Diagnostics.Stopwatch]::StartNew()
        while ($swConfig.ElapsedMilliseconds -lt 20000) {
            if (Test-Path $fullOutput) {
                $len = (Get-Item $fullOutput).Length
                if ($len -gt 0) { break }
            }
            $wins = [PdnWin32]::Top($proc.Id)
            foreach ($w in $wins) {
                if ($w.Hwnd -eq $mainWin.Hwnd -or $w.ClassName -like "UAC*" -or $w.ClassName -eq "#32770") { continue }
                if ($w.Title -in $panelTitles) { continue }
                if ($w.Title -match "Save Options|Save Configuration|Salvar Configura|Opções de Salvamento|Configura") {
                    Write-Host ("  Config dialog detected: '" + $w.Title + "'")
                    if (-not $bitDepthSet) {
                        Start-Sleep -Milliseconds 250
                        $picked = [PdnWin32]::SelectComboItemByRegex($w.Hwnd, "32")
                        Write-Host ("  Selected PNG Bit Depth: '" + $picked + "'")
                        $bitDepthSet = $true
                        Start-Sleep -Milliseconds 600
                    }
                    $okChild = [PdnWin32]::Children($w.Hwnd) | Where-Object { $_.Title -eq "OK" -or $_.Title -eq "&OK" } | Select-Object -First 1
                    if ($okChild) {
                        [PdnWin32]::ClickButton($okChild.Hwnd)
                        Start-Sleep -Milliseconds 600
                        break
                    } else {
                        $cfgEl = [System.Windows.Automation.AutomationElement]::FromHandle($w.Hwnd)
                        $okEl = $cfgEl.FindFirst(
                            [System.Windows.Automation.TreeScope]::Descendants,
                            (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, "OK"))
                        )
                        if ($okEl) {
                            $r = $okEl.Current.BoundingRectangle
                            [PdnWin32]::ClickScreenPointInWindow($w.Hwnd, [int]($r.Left + $r.Width / 2), [int]($r.Top + $r.Height / 2))
                            Start-Sleep -Milliseconds 600
                            break
                        }
                    }
                } else {
                    # Handle Flatten confirmation or TaskDialog modal
                    Write-Host ("  Confirmation modal detected: '" + $w.Title + "' (Class=" + $w.ClassName + ")")
                    $btn = [PdnWin32]::Children($w.Hwnd) | Where-Object { $_.Title -match "^(&?Flatten|&?Achatar|&?Yes|&?Sim|&?OK)$" } | Select-Object -First 1
                    if ($btn) {
                        Write-Host ("  Clicking modal button: '" + $btn.Title + "'")
                        [PdnWin32]::ClickButton($btn.Hwnd)
                        Start-Sleep -Milliseconds 600
                        break
                    } else {
                        $dlgEl = [System.Windows.Automation.AutomationElement]::FromHandle($w.Hwnd)
                        $btns = $dlgEl.FindAll(
                            [System.Windows.Automation.TreeScope]::Descendants,
                            (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::Button))
                        )
                        $clicked = $false
                        foreach ($b in $btns) {
                            if ($b.Current.Name -match "Flatten|Achatar|^Yes$|^Sim$|^OK$") {
                                Write-Host ("  Clicking UIA modal button: '" + $b.Current.Name + "'")
                                $r = $b.Current.BoundingRectangle
                                [PdnWin32]::ClickScreenPointInWindow($w.Hwnd, [int]($r.Left + $r.Width / 2), [int]($r.Top + $r.Height / 2))
                                $clicked = $true
                                Start-Sleep -Milliseconds 600
                                break
                            }
                        }
                        if ($clicked) { break }
                    }
                }
            }
            Start-Sleep -Milliseconds 200
        }

        [void](Wait-Until -TimeoutMs 15000 -PollMs 150 -FailureMessage "Output file '$fullOutput' was not written." -Condition {
            if (-not (Test-Path $fullOutput)) { return $false }
            $len1 = (Get-Item $fullOutput).Length
            if ($len1 -le 8) { return $false }
            Start-Sleep -Milliseconds 300
            $len2 = (Get-Item $fullOutput).Length
            return ($len1 -eq $len2)
        })

        Assert-MagicBytes -Path $fullOutput -TargetFormat $TargetFormat
        Write-Host "Verified $fullOutput ($((Get-Item $fullOutput).Length) bytes)."
    }
    catch {
        $hadError = $true
        Write-Host ("ERROR: " + $_.Exception.Message)
        throw
    }
    finally {
        Start-Sleep -Milliseconds 300
        if (-not $proc.HasExited) {
            if ($mainWin) {
                [PdnWin32]::CloseWindow($mainWin.Hwnd)
            }
            if (-not $proc.WaitForExit(8000)) {
                if ($hadError) {
                    Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
                } else {
                    throw "Paint.NET did not close cleanly after saving '$fullOutput'."
                }
            }
        }
        Start-Sleep -Milliseconds 500
    }
}

$resolvedDir = (Resolve-Path $FixturesDir).Path

Export-ViaPaintNet52 `
    -InputPdn (Join-Path $resolvedDir "pdn52_blend_mosaic.pdn") `
    -OutputPath (Join-Path $resolvedDir "pdn52_blend_mosaic-pdn52.png") `
    -TargetFormat "PNG"

Export-ViaPaintNet52 `
    -InputPdn (Join-Path $resolvedDir "FlattenBlendTest.pdn") `
    -OutputPath (Join-Path $resolvedDir "FlattenBlendTest-pdn52.png") `
    -TargetFormat "PNG"

Export-ViaPaintNet52 `
    -InputPdn (Join-Path $resolvedDir "pdn52_roundtrip_layers.pdn") `
    -OutputPath (Join-Path $resolvedDir "pdn52_roundtrip_layers-pdn52.png") `
    -TargetFormat "PNG"

Export-ViaPaintNet52 `
    -InputPdn (Join-Path $resolvedDir "pdn52_roundtrip_layers.pdn") `
    -OutputPath (Join-Path $resolvedDir "pdn52_roundtrip_resaved.pdn") `
    -TargetFormat "PDN"

Write-Host "All Paint.NET 5.2 reference outputs generated cleanly."
