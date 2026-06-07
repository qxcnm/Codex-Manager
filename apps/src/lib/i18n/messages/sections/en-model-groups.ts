"use client";

import type { MessageCatalog } from "../types";

export const EN_MODEL_GROUPS_MESSAGES: MessageCatalog = {
  "模型组已保存": "Model group saved",
  "模型组已删除": "Model group deleted",
  "模型权限已保存": "Model permissions saved",
  "成员分配已保存": "Member assignment saved",
  "确认删除该模型组？": "Delete this model group?",
  "只有管理员可以管理模型组": "Only administrators can manage model groups",
  "按用户分配可用平台模型，并为不同订阅层配置扣费倍率。":
    "Assign available platform models by user and configure billing multipliers for different subscription tiers.",
  "新建模型组": "New model group",
  "模型组列表": "Model group list",
  "在列表中查看订阅层配置，具体模型权限和成员分配通过弹窗维护。":
    "Review subscription-tier settings in the list. Model permissions and member assignments are managed in the dialog.",
  "模型权限": "Model permissions",
  成员: "Members",
  倍率: "Multiplier",
  更新时间: "Updated at",
  "加载中...": "Loading...",
  "暂无模型组": "No model groups",
  "未填写描述": "No description",
  管理: "Manage",
  "模型组操作": "Model group actions",
  基础信息: "Basic info",
  成员分配: "Member assignment",
  "管理模型组": "Manage model group",
  "先保存基础信息，再继续配置模型权限和成员。":
    "Save the basic information first, then configure model permissions and members.",
  个模型: "models",
  个成员: "members",
  描述: "Description",
  默认倍率: "Default multiplier",
  "设为新成员默认模型组": "Set as the default model group for new members",
  "保存基础信息": "Save basic info",
  "保存并继续": "Save and continue",
  "启用后，该组成员才能调用对应平台模型；倍率为空时使用模型组默认倍率。":
    "After enabling a model, members in this group can call that platform model. Empty multipliers use the model group's default.",
  "保存模型": "Save models",
  平台模型: "Platform model",
  计费模型: "Billing model",
  "暂无平台模型": "No platform models",
  "成员可同时持有多个模型组，最终按可用模型集合和最低有效倍率生效。":
    "Members can belong to multiple model groups. The final result uses the union of available models and the lowest effective multiplier.",
  "保存成员": "Save members",
  "暂无可分配成员": "No assignable members",
};
