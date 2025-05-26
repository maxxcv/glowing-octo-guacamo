<template>
  <el-container style="height:100vh">
    <!-- 左侧导航 -->
    <el-aside width="200px" class="sidebar">
      <div class="logo">🚀 下载管理</div>
      <el-menu :default-active="filter" @select="filter = $event">
        <el-menu-item index="all">全部任务</el-menu-item>
        <el-menu-item index="running">下载中</el-menu-item>
        <el-menu-item index="done">已完成</el-menu-item>
        <el-menu-item index="error">出错</el-menu-item>
      </el-menu>
    </el-aside>

    <el-container>
      <!-- 顶部工具栏 -->
      <el-header class="toolbar">
        <el-input
          v-model="search"
          placeholder="搜索任务"
          prefix-icon="el-icon-search"
          clearable
          class="toolbar-search"
          @clear="search=''"
        />
        <el-space class="toolbar-actions">
          <el-button icon="el-icon-sunrise" @click="toggleTheme" circle/>
          <el-avatar icon="el-icon-user" circle/>
        </el-space>
      </el-header>

      <!-- 主内容区：添加任务 + 列表 -->
      <el-main class="main-content">
        <div class="add-task">
          <el-input
            v-model="newUrl"
            placeholder="输入下载链接，按 Enter 添加"
            clearable
            @keyup.enter="addTask"
            style="width:60%; margin-right:8px;"
          />
          <el-button type="primary" @click="addTask">添加下载</el-button>
        </div>
        <el-row :gutter="20" style="margin-top:16px;">
          <el-col :span="6" v-for="task in filteredTasks" :key="task.id">
            <el-card class="task-card">
              <!-- 卡片头部 -->
              <div class="header">
                <el-icon><Download /></el-icon>
                <span class="title">{{ task.name }}</span>
                <el-badge
                  :value="stateLabel(task.state)"
                  :type="stateType(task.state)"
                  class="badge"
                />
              </div>
              <!-- 进度条 -->
              <el-progress
                :percentage="Math.floor(task.progress)"
                :status="task.state==='error'?'exception':'success'"
                stroke-width="18"
                :text-inside="true"
                :format="() => Math.floor(task.progress) + '%'"
              />
              <!-- 底部：速度 + 操作按钮 -->
              <div class="footer">
                <div class="speed">{{ formatSpeed(task.speed) }}</div>
                <el-space>
                  <el-button
                    size="mini"
                    type="primary"
                    @click="startTask(task)"
                    :disabled="task.state!=='idle'"
                  >开始</el-button>
                  <el-button
                    size="mini"
                    type="warning"
                    @click="pauseTask(task)"
                    v-if="task.state==='running'"
                  >暂停</el-button>
                  <el-button
                    size="mini"
                    type="success"
                    @click="resumeTask(task)"
                    v-if="task.state==='paused'"
                  >恢复</el-button>
                </el-space>
              </div>
            </el-card>
          </el-col>
        </el-row>
      </el-main>
    </el-container>
  </el-container>
</template>

<script setup lang="ts">
import { ref, reactive, computed, onMounted } from 'vue';
import { invoke } from '@tauri-apps/api/tauri';
import { listen } from '@tauri-apps/api/event';
import PQueue from 'p-queue';
import { Download } from '@element-plus/icons-vue';

// 任务数据类型
interface Task {
  id: string;
  url: string;
  name: string;
  progress: number;
  speed: number;     // B/s
  state: 'idle'|'running'|'paused'|'done'|'error';
}

// 并发队列，限 3 个同时下载
const queue = new PQueue({ concurrency: 3 });

// 响应式状态
const tasks = reactive<Task[]>([]);
const newUrl = ref('');
const search = ref('');
const filter = ref<'all'|'running'|'done'|'error'>('all');
const theme = ref<'light'|'dark'>('light');

// 过滤后列表
const filteredTasks = computed(() =>
  tasks.filter(t =>
    (filter.value === 'all' || t.state === filter.value) &&
    t.name.includes(search.value)
  )
);

// 添加任务
function addTask() {
  if (!newUrl.value) return;
  const id = Date.now().toString();
  tasks.push({
    id,
    url: newUrl.value,
    name: newUrl.value.split('/').pop() || id,
    progress: 0,
    speed: 0,
    state: 'idle'
  });
  newUrl.value = '';
}

// 控制下载
function startTask(task: Task) {
  task.state = 'running';
  queue.add(() => handleDownload(task));
}
function pauseTask(task: Task) {
  invoke('cancel_download', { id: task.id });
  task.state = 'paused';
}
function resumeTask(task: Task) {
  task.state = 'running';
  queue.add(() => handleDownload(task));
}

// 真正调用后端
async function handleDownload(task: Task) {
  try {
    await invoke('download', { id: task.id, url: task.url, output: `${task.id}.bin` });
    task.state = 'done';
  } catch (e) {
    task.state = (`${e}` === 'Paused') ? 'paused' : 'error';
  }
}

// 监听后端进度推送
onMounted(async () => {
  await listen<{
    download_id: string;
    transferred: number;
    transfer_rate: number;
    percentage: number;
  }>('DOWNLOAD_PROGRESS', ({ payload }) => {
    const t = tasks.find(x => x.id === payload.download_id);
    if (!t) return;
    t.progress = payload.percentage;
    t.speed = payload.transfer_rate;
    if (payload.percentage >= 100) t.state = 'done';
  });
});

// 辅助格式化
function formatSpeed(bps: number) {
  if (bps > 1024*1024) return (bps/1024/1024).toFixed(2) + ' MB/s';
  if (bps > 1024) return (bps/1024).toFixed(1) + ' KB/s';
  return bps.toFixed(0) + ' B/s';
}
function stateLabel(s: Task['state']) {
  return { idle:'待下载', running:'下载中', paused:'已暂停', done:'完成', error:'出错' }[s];
}
function stateType(s: Task['state']) {
  return { idle:'info', running:'primary', paused:'warning', done:'success', error:'danger' }[s];
}

// 主题切换
function toggleTheme() {
  theme.value = theme.value === 'light' ? 'dark' : 'light';
  document.documentElement.setAttribute('data-theme', theme.value);
}
</script>

<style scoped>
.sidebar {
  background: var(--card-bg);
  border-right: 1px solid #dcdfe6;
}
.logo {
  padding: 16px;
  font-size: 1.2em;
  text-align: center;
}
.toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  background: var(--card-bg);
  border-bottom: 1px solid #dcdfe6;
  padding: 0 16px;
}
.toolbar-search { width: 300px; }
.toolbar-actions { gap: 12px; }
.main-content { padding: 16px; overflow-y: auto; }
.add-task { display: flex; align-items: center; }
.task-card { background: var(--card-bg); border-radius: 8px; }
.header {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 8px;
}
.title { flex: 1; font-weight: 500; }
.badge { margin-left: auto; }
.footer {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-top: 12px;
}
.speed { font-size: 0.9em; color: #909399; }
</style>
